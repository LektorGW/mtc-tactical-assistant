use super::native::{client_bounds, client_crop, hwnd, validate_window, CropRect, DpiScope};
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, SyncSender},
    Arc, Condvar, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::{
    core::{factory, IInspectable, Interface},
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession, IDirect3D11CaptureFramePoolStatics2,
            IGraphicsCaptureSessionStatics,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::*,
            Dxgi::{Common::*, IDXGIDevice},
        },
        System::{
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
                RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED,
            },
        },
    },
};

const MAX_FRAME_AGE: Duration = Duration::from_secs(1);
const STATUS_INTERVAL: Duration = Duration::from_millis(50);
const DEFAULT_COPY_INTERVAL: Duration = Duration::from_millis(333);
const MAX_FUTURE_DEFER: Duration = Duration::from_millis(200);
const PIXEL_FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

#[derive(Default)]
struct Latest {
    frame: Option<(CapturedFrame, Instant)>,
    error: Option<CaptureError>,
    stopped: bool,
}

type Shared = Arc<(Mutex<Latest>, Condvar)>;

/// Persistent WGC session. All COM and Direct3D work runs on one dedicated thread.
/// The worker replaces one latest image; it never queues image history or writes files.
pub struct WindowCaptureSession {
    handle: i64,
    shared: Shared,
    stop: Arc<AtomicBool>,
    wake: SyncSender<()>,
    worker: Option<JoinHandle<()>>,
    last_sequence: u64,
    interval_ms: Arc<AtomicU64>,
}

impl WindowCaptureSession {
    pub fn start(handle: i64) -> Result<Self, CaptureError> {
        Self::start_with_interval(handle, DEFAULT_COPY_INTERVAL)
    }

    pub fn start_with_interval(handle: i64, interval: Duration) -> Result<Self, CaptureError> {
        validate_window(handle)?;
        let shared = Arc::new((Mutex::new(Latest::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, wake_rx) = mpsc::sync_channel(1);
        let (ready, ready_rx) = mpsc::sync_channel(1);
        let worker_shared = Arc::clone(&shared);
        let worker_stop = Arc::clone(&stop);
        let worker_wake = wake.clone();
        let interval_ms = Arc::new(AtomicU64::new(interval_millis(interval)));
        let worker_interval = Arc::clone(&interval_ms);
        let worker = thread::Builder::new()
            .name("mtc-wgc".into())
            .spawn(move || {
                run_worker(
                    handle,
                    worker_shared,
                    worker_stop,
                    worker_wake,
                    wake_rx,
                    ready,
                    worker_interval,
                )
            })
            .map_err(|e| CaptureError::Backend(format!("Nie udało się uruchomić WGC: {e}")))?;
        let mut session = Self {
            handle,
            shared,
            stop,
            wake,
            worker: Some(worker),
            last_sequence: 0,
            interval_ms,
        };
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(session),
            Ok(Err(error)) => {
                session.stop()?;
                Err(error)
            }
            Err(_) => {
                session.stop.store(true, Ordering::Release);
                let _ = session.wake.try_send(());
                // Do not block the caller on a GPU driver's unresponsive initialization.
                session.worker.take();
                Err(CaptureError::Backend(
                    "Przekroczono czas uruchamiania Windows Graphics Capture".into(),
                ))
            }
        }
    }

    pub fn handle(&self) -> i64 {
        self.handle
    }

    /// Controls actual GPU readbacks, not only consumer polling. Clamped to 16..=10000 ms.
    pub fn set_frame_interval(&mut self, interval: Duration) {
        self.interval_ms
            .store(interval_millis(interval), Ordering::Release);
        let _ = self.wake.try_send(());
    }

    /// Returns a new source frame, at most one second old. A duplicate never gets a fresh timestamp.
    /// Hidden/minimized/closed windows fail immediately instead of returning the retained image.
    pub fn next_frame(&mut self, timeout: Duration) -> Result<CapturedFrame, CaptureError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(CaptureError::Timeout)?;
        loop {
            if let Err(error) = validate_window(self.handle) {
                set_error(&self.shared, error.clone());
                return Err(error);
            }
            let (_, client_width, client_height) = client_bounds(self.handle)?;
            let worker_finished = self
                .worker
                .as_ref()
                .is_none_or(|worker| worker.is_finished());
            let (lock, changed) = &*self.shared;
            let mut latest = lock.lock().unwrap_or_else(|e| e.into_inner());
            if latest.stopped || self.stop.load(Ordering::Acquire) {
                return Err(CaptureError::Stopped);
            }
            if let Some(error) = &latest.error {
                // The window is visible now. Let the worker restart a suspended pipeline.
                if !matches!(
                    error,
                    CaptureError::Hidden | CaptureError::Minimized | CaptureError::GeometryChanged
                ) {
                    return Err(error.clone());
                }
            }
            if let Some(frame) = take_fresh_frame(
                &mut latest,
                self.last_sequence,
                (client_width, client_height),
                Instant::now(),
            ) {
                self.last_sequence = frame.sequence;
                return Ok(frame);
            }
            if worker_finished {
                // A worker that exited (or panicked) never produces another frame.
                // Report it instead of timing out forever on a dead session.
                return Err(finished_worker_error(&latest));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CaptureError::Timeout);
            }
            let _ = changed
                .wait_timeout(latest, remaining.min(STATUS_INTERVAL))
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Stops the worker and releases capture, event handlers, staging texture and frame pool.
    /// Safe to call repeatedly; Drop also calls this method.
    pub fn stop(&mut self) -> Result<(), CaptureError> {
        self.stop.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
        {
            let mut latest = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
            latest.frame = None;
            latest.stopped = true;
            self.shared.1.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| CaptureError::Backend("Wątek WGC zakończył się błędem".into()))?;
        }
        Ok(())
    }
}

impl Drop for WindowCaptureSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct WinRt;
impl WinRt {
    fn initialize() -> Result<Self, CaptureError> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(api_error)?;
        Ok(Self)
    }
}
impl Drop for WinRt {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

fn set_error(shared: &Shared, error: CaptureError) {
    let mut latest = shared.0.lock().unwrap_or_else(|e| e.into_inner());
    latest.frame = None;
    latest.error = Some(error);
    shared.1.notify_all();
}

fn finished_worker_error(latest: &Latest) -> CaptureError {
    match &latest.error {
        Some(error)
            if !matches!(
                error,
                CaptureError::Hidden | CaptureError::Minimized | CaptureError::GeometryChanged
            ) =>
        {
            error.clone()
        }
        _ => CaptureError::Backend(
            "Wątek Windows Graphics Capture zakończył pracę; uruchom przechwytywanie ponownie"
                .into(),
        ),
    }
}

fn interval_millis(interval: Duration) -> u64 {
    interval.as_millis().clamp(16, 10_000) as u64
}

fn take_fresh_frame(
    latest: &mut Latest,
    previous: u64,
    dimensions: (u32, u32),
    now: Instant,
) -> Option<CapturedFrame> {
    let (frame, source_time) = latest.frame.take()?;
    (frame.sequence > previous
        && now.saturating_duration_since(source_time) <= MAX_FRAME_AGE
        && frame.image.dimensions() == dimensions)
        .then_some(frame)
}

fn run_worker(
    handle: i64,
    shared: Shared,
    stop: Arc<AtomicBool>,
    wake: SyncSender<()>,
    wake_rx: Receiver<()>,
    ready: SyncSender<Result<(), CaptureError>>,
    interval_ms: Arc<AtomicU64>,
) {
    let _dpi = DpiScope::enter();
    let runtime = match WinRt::initialize() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let closed = Arc::new(AtomicBool::new(false));
    let mut pipeline = match Pipeline::create(handle, wake.clone(), Arc::clone(&closed)) {
        Ok(pipeline) => pipeline,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if stop.load(Ordering::Acquire) {
        return;
    }
    let _ = ready.send(Ok(()));
    let mut sequence = 0u64;
    let mut last_copy = Instant::now()
        .checked_sub(Duration::from_millis(interval_ms.load(Ordering::Acquire)))
        .unwrap_or_else(Instant::now);
    let mut last_future_log = None::<Instant>;
    while !stop.load(Ordering::Acquire) {
        let _ = wake_rx.recv_timeout(STATUS_INTERVAL);
        if stop.load(Ordering::Acquire) {
            break;
        }
        if closed.load(Ordering::Acquire) {
            set_error(&shared, CaptureError::Closed);
            break;
        }
        if let Err(error) = validate_window(handle) {
            set_error(&shared, error.clone());
            if error == CaptureError::Closed {
                break;
            }
            continue;
        }
        let resumed = {
            let mut latest = shared.0.lock().unwrap_or_else(|e| e.into_inner());
            let resumed = matches!(
                latest.error,
                Some(CaptureError::Hidden | CaptureError::Minimized)
            );
            if matches!(
                latest.error,
                Some(
                    CaptureError::Hidden | CaptureError::Minimized | CaptureError::GeometryChanged
                )
            ) {
                latest.error = None;
            }
            resumed
        };
        if resumed {
            // Some compositors keep returning a black surface after Hide/Show.
            // A new public capture session discards every surface from the hidden state.
            match Pipeline::create(handle, wake.clone(), Arc::clone(&closed)) {
                Ok(new_pipeline) => pipeline = new_pipeline,
                Err(error) => {
                    set_error(&shared, error);
                    break;
                }
            }
        }
        // GPU readback follows the consumer's interval even if the source renders at 120+ fps.
        // The two-buffer OS pool stops producing frames once both surfaces wait in its queue,
        // so between copies only the newest surface stays checked out and older ones are
        // returned. Otherwise every copy would be as old as the interval and a >1 s interval
        // (0.25-1 FPS analysis) could never deliver a fresh frame.
        if last_copy.elapsed() < Duration::from_millis(interval_ms.load(Ordering::Acquire)) {
            if let Err(error) = pipeline.retain_newest() {
                set_error(&shared, error);
                break;
            }
            continue;
        }
        match pipeline.poll(handle) {
            Ok(Some((image, timestamp, age))) => {
                last_copy = Instant::now();
                if age > MAX_FRAME_AGE {
                    continue;
                }
                sequence = sequence.saturating_add(1);
                let now = Instant::now();
                let captured_at = now.checked_sub(age).unwrap_or(now);
                let frame = CapturedFrame {
                    image,
                    backend: CaptureBackend::WindowsGraphicsCapture,
                    captured_at_ms: unix_time_ms().saturating_sub(age.as_millis() as u64),
                    source_timestamp_100ns: Some(timestamp),
                    sequence,
                };
                let mut latest = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                latest.frame = Some((frame, captured_at));
                latest.error = None;
                shared.1.notify_all();
            }
            Ok(None) => {}
            Err(CaptureError::GeometryChanged) => {
                // A mismatched transitional frame is dropped, never silently shifted or stretched.
                let mut latest = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                latest.frame = None;
            }
            Err(error @ CaptureError::FutureFrame { .. }) => {
                // Invalid time is local to one frame. Do not poison the persistent session.
                let now = Instant::now();
                if last_future_log
                    .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(1))
                {
                    eprintln!("WGC_CLOCK_RETRY {error}");
                    last_future_log = Some(now);
                }
                let mut latest = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                latest.frame = None;
                shared.1.notify_all();
            }
            Err(error @ (CaptureError::Hidden | CaptureError::Minimized)) => {
                // Visibility may change between the status check and the pixel copy.
                set_error(&shared, error);
            }
            Err(error) => {
                set_error(&shared, error);
                break;
            }
        }
    }
    // Release Windows objects before the WinRT apartment is uninitialized.
    drop(pipeline);
    drop(runtime);
}

fn api_error(error: windows::core::Error) -> CaptureError {
    CaptureError::Backend(format!("Windows Graphics Capture: {error}"))
}

fn capture_supported() -> Result<bool, CaptureError> {
    // The generated static method caches its factory across apartment teardown.
    // A session-local factory must be released before this worker calls RoUninitialize.
    let factory =
        factory::<GraphicsCaptureSession, IGraphicsCaptureSessionStatics>().map_err(api_error)?;
    let mut supported = false;
    unsafe { (Interface::vtable(&factory).IsSupported)(factory.as_raw(), &mut supported).ok() }
        .map_err(api_error)?;
    Ok(supported)
}

fn create_frame_pool(
    device: &IDirect3DDevice,
    size: SizeInt32,
) -> Result<Direct3D11CaptureFramePool, CaptureError> {
    let factory = factory::<Direct3D11CaptureFramePool, IDirect3D11CaptureFramePoolStatics2>()
        .map_err(api_error)?;
    let mut result = std::ptr::null_mut();
    unsafe {
        (Interface::vtable(&factory).CreateFreeThreaded)(
            factory.as_raw(),
            device.as_raw(),
            PIXEL_FORMAT,
            2,
            size,
            &mut result,
        )
        .ok()
    }
    .map_err(api_error)?;
    if result.is_null() {
        return Err(CaptureError::Backend(
            "Windows Graphics Capture nie zwróciło puli klatek".into(),
        ));
    }
    // The successful ABI call transfers one owning COM reference to the caller.
    Ok(unsafe { Direct3D11CaptureFramePool::from_raw(result) })
}

struct Pipeline {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    multithread: ID3D11Multithread,
    runtime_device: IDirect3DDevice,
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    frame_token: Option<i64>,
    closed_token: Option<i64>,
    pool_size: SizeInt32,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    qpc_frequency: i64,
    pending: Option<(CheckedOutFrames, Instant)>,
    /// Newest checked-out surface that has not been copied yet. One of the two pool
    /// buffers stays free for the compositor, so the next frame can always arrive.
    newest: Option<CheckedOutFrames>,
}

impl Pipeline {
    fn create(
        handle: i64,
        wake: SyncSender<()>,
        closed: Arc<AtomicBool>,
    ) -> Result<Self, CaptureError> {
        if !capture_supported()? {
            return Err(CaptureError::Unsupported);
        }
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(api_error)?;
        }
        let device =
            device.ok_or_else(|| CaptureError::Backend("Brak urządzenia Direct3D11".into()))?;
        let context =
            context.ok_or_else(|| CaptureError::Backend("Brak kontekstu Direct3D11".into()))?;
        // WGC's free-threaded compositor can use the same immediate context.
        // Protect it as in Microsoft's capture sample; do not rely on our single worker alone.
        let multithread: ID3D11Multithread = context.cast().map_err(api_error)?;
        unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        }
        let dxgi: IDXGIDevice = device.cast().map_err(api_error)?;
        let runtime_device: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
                .and_then(|device| device.cast())
                .map_err(api_error)?;
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(api_error)?;
        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForWindow(hwnd(handle)) }.map_err(api_error)?;
        let pool_size = item.Size().map_err(api_error)?;
        validate_content_size(pool_size)?;
        let pool = create_frame_pool(&runtime_device, pool_size)?;
        let session = pool.CreateCaptureSession(&item).map_err(api_error)?;
        let mut qpc_frequency = 0;
        unsafe { QueryPerformanceFrequency(&mut qpc_frequency) }.map_err(api_error)?;
        let mut pipeline = Self {
            device,
            context,
            multithread,
            runtime_device,
            item,
            pool,
            session,
            frame_token: None,
            closed_token: None,
            pool_size,
            staging: None,
            qpc_frequency,
            pending: None,
            newest: None,
        };
        // Failure to disable the cursor is explicit, rather than contaminating OCR/markers.
        pipeline
            .session
            .SetIsCursorCaptureEnabled(false)
            .map_err(api_error)?;
        // Keep Windows' visible capture border. No access request to hide it is made.
        let frame_wake = wake.clone();
        pipeline.frame_token = Some(
            pipeline
                .pool
                .FrameArrived(
                    &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                        move |_, _| {
                            let _ = frame_wake.try_send(());
                            Ok(())
                        },
                    ),
                )
                .map_err(api_error)?,
        );
        pipeline.closed_token = Some(
            pipeline
                .item
                .Closed(
                    &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                        closed.store(true, Ordering::Release);
                        let _ = wake.try_send(());
                        Ok(())
                    }),
                )
                .map_err(api_error)?,
        );
        pipeline.session.StartCapture().map_err(api_error)?;
        Ok(pipeline)
    }

    /// Returns every older queued surface to the pool and keeps only the newest one.
    /// No GPU copy is made here; an older surface is closed only after a newer one
    /// has been checked out, and never while a deferred future surface is pending.
    fn retain_newest(&mut self) -> Result<(), CaptureError> {
        if self.pending.is_some() {
            return Ok(());
        }
        loop {
            match self.pool.TryGetNextFrame() {
                Ok(frame) => {
                    if let Some(previous) = self.newest.take() {
                        if previous.0.iter().any(|old| old.as_raw() == frame.as_raw()) {
                            // The same COM frame is already held; keep a single owner.
                            self.newest = Some(previous);
                            return Ok(());
                        }
                        // Closing the older surface returns its buffer to the pool.
                        drop(previous);
                    }
                    self.newest = Some(CheckedOutFrames(vec![frame]));
                }
                // WinRT succeeds with a null frame when the pool is empty; windows-rs exposes Error::empty.
                Err(error) if error.code().0 == 0 => return Ok(()),
                Err(error) => return Err(api_error(error)),
            }
        }
    }

    fn poll(&mut self, handle: i64) -> Result<Option<(RgbaImage, i64, Duration)>, CaptureError> {
        let (frames, first_seen) = if let Some(pending) = self.pending.take() {
            pending
        } else {
            self.retain_newest()?;
            match self.newest.take() {
                Some(frames) => (frames, Instant::now()),
                None => return Ok(None),
            }
        };
        let frame = frames.0.last().expect("Nonempty checked-out frame batch");
        let timestamp = frame.SystemRelativeTime().map_err(api_error)?.Duration;
        let mut qpc_now = 0;
        unsafe { QueryPerformanceCounter(&mut qpc_now) }.map_err(api_error)?;
        match source_age(qpc_now, self.qpc_frequency, timestamp) {
            Err(error) => {
                if let CaptureError::FutureFrame { delta_100ns, .. } = &error {
                    if can_defer_future(*delta_100ns, first_seen.elapsed()) {
                        self.pending = Some((frames, first_seen));
                    }
                }
                return Err(error);
            }
            // A surface already older than the limit is returned without a GPU copy;
            // the worker waits for the next source frame instead of a full interval.
            Ok(age) if age > MAX_FRAME_AGE => return Ok(None),
            Ok(_) => {}
        }
        let size = frame.ContentSize().map_err(api_error)?;
        validate_content_size(size)?;
        if size != self.pool_size {
            drop(frames);
            self.pool
                .Recreate(&self.runtime_device, PIXEL_FORMAT, 2, size)
                .map_err(api_error)?;
            self.pool_size = size;
            self.staging = None;
            return Ok(None);
        }
        let crop = client_crop(handle, size.Width as u32, size.Height as u32)?;
        let surface = frame.Surface().map_err(api_error)?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(api_error)?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(api_error)?;
        let mut description = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut description) };
        crop.validate(description.Width, description.Height)?;
        let image = self.copy_client(&texture, crop)?;
        let mut qpc_now = 0;
        unsafe { QueryPerformanceCounter(&mut qpc_now) }.map_err(api_error)?;
        let age = source_age(qpc_now, self.qpc_frequency, timestamp)?;
        Ok(Some((image, timestamp, age)))
    }

    fn copy_client(
        &mut self,
        source: &ID3D11Texture2D,
        crop: CropRect,
    ) -> Result<RgbaImage, CaptureError> {
        let _context_lock = ContextLock::enter(&self.multithread);
        if self
            .staging
            .as_ref()
            .is_none_or(|(_, w, h)| *w != crop.width || *h != crop.height)
        {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: crop.width,
                Height: crop.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            };
            let mut texture = None;
            unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }
                .map_err(api_error)?;
            self.staging = Some((
                texture.ok_or_else(|| CaptureError::Backend("Brak bufora klatki".into()))?,
                crop.width,
                crop.height,
            ));
        }
        let staging = &self.staging.as_ref().expect("Created staging texture").0;
        let region = D3D11_BOX {
            left: crop.x as u32,
            top: crop.y as u32,
            front: 0,
            right: crop.x as u32 + crop.width,
            bottom: crop.y as u32 + crop.height,
            back: 1,
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .CopySubresourceRegion(staging, 0, 0, 0, 0, source, 0, Some(&region));
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(api_error)?;
        }
        let _mapping = MappedGuard {
            context: &self.context,
            texture: staging,
        };
        if mapped.pData.is_null() || mapped.RowPitch < crop.width * 4 {
            return Err(CaptureError::Backend("Niepoprawny bufor GPU".into()));
        }
        // Every row is read only to width*4; padding is excluded from the final image.
        // The last row is not required to carry trailing pitch padding.
        let length = (mapped.RowPitch as usize)
            .checked_mul(crop.height as usize - 1)
            .and_then(|rows| rows.checked_add(crop.width as usize * 4))
            .ok_or(CaptureError::InvalidSize)?;
        let bytes = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), length) };
        bgra_rows_to_rgba(bytes, mapped.RowPitch as usize, crop.width, crop.height)
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        // Return deferred and retained checked-out surfaces before closing the pool/session.
        self.pending = None;
        self.newest = None;
        if let Some(token) = self.frame_token.take() {
            let _ = self.pool.RemoveFrameArrived(token);
        }
        if let Some(token) = self.closed_token.take() {
            let _ = self.item.RemoveClosed(token);
        }
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

struct CheckedOutFrames(Vec<Direct3D11CaptureFrame>);
impl Drop for CheckedOutFrames {
    fn drop(&mut self) {
        for (index, frame) in self.0.iter().enumerate() {
            if !self.0[..index]
                .iter()
                .any(|earlier| earlier.as_raw() == frame.as_raw())
            {
                let _ = frame.Close();
            }
        }
    }
}

struct ContextLock<'a>(&'a ID3D11Multithread);
impl<'a> ContextLock<'a> {
    fn enter(context: &'a ID3D11Multithread) -> Self {
        unsafe {
            context.Enter();
        }
        Self(context)
    }
}
impl Drop for ContextLock<'_> {
    fn drop(&mut self) {
        unsafe {
            self.0.Leave();
        }
    }
}

struct MappedGuard<'a> {
    context: &'a ID3D11DeviceContext,
    texture: &'a ID3D11Texture2D,
}
impl Drop for MappedGuard<'_> {
    fn drop(&mut self) {
        unsafe { self.context.Unmap(self.texture, 0) };
    }
}

fn validate_content_size(size: SizeInt32) -> Result<(), CaptureError> {
    if size.Width <= 0 || size.Height <= 0 || size.Width > 7800 || size.Height > 4500 {
        return Err(CaptureError::InvalidSize);
    }
    Ok(())
}

fn source_age(
    qpc_now: i64,
    qpc_frequency: i64,
    timestamp_100ns: i64,
) -> Result<Duration, CaptureError> {
    if qpc_frequency <= 0 || qpc_now < 0 || timestamp_100ns < 0 {
        return Err(CaptureError::Backend(
            "Niepoprawny znacznik czasu klatki WGC".into(),
        ));
    }
    let now_100ns = qpc_now as i128 * 10_000_000 / qpc_frequency as i128;
    let age_100ns = now_100ns - timestamp_100ns as i128;
    if std::env::var_os("MTC_CAPTURE_CLOCK_DIAGNOSTICS").is_some() {
        eprintln!("WGC_CLOCK qpc_now={qpc_now} qpc_frequency={qpc_frequency} source_timestamp_100ns={timestamp_100ns} now_100ns={now_100ns} delta_100ns={age_100ns}");
    }
    if age_100ns < 0 {
        return Err(CaptureError::FutureFrame {
            qpc_now,
            qpc_frequency,
            source_timestamp_100ns: timestamp_100ns,
            delta_100ns: age_100ns,
        });
    }
    let age_nanos = age_100ns.saturating_mul(100).min(u64::MAX as i128) as u64;
    Ok(Duration::from_nanos(age_nanos))
}

fn can_defer_future(delta_100ns: i128, waited: Duration) -> bool {
    delta_100ns < 0
        && delta_100ns >= -(MAX_FUTURE_DEFER.as_nanos() as i128 / 100)
        && waited < MAX_FUTURE_DEFER
}

fn bgra_rows_to_rgba(
    bytes: &[u8],
    pitch: usize,
    width: u32,
    height: u32,
) -> Result<RgbaImage, CaptureError> {
    let row_size = width as usize * 4;
    let required = pitch
        .checked_mul((height as usize).saturating_sub(1))
        .and_then(|rows| rows.checked_add(row_size));
    if width == 0 || height == 0 || pitch < row_size || required.is_none_or(|n| bytes.len() < n) {
        return Err(CaptureError::InvalidSize);
    }
    // One allocation per delivered frame; rows are swizzled in place without
    // per-pixel vector growth.
    let mut pixels = vec![0u8; row_size * height as usize];
    for (target, source) in pixels.chunks_exact_mut(row_size).zip(bytes.chunks(pitch)) {
        for (out, pixel) in target
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(source[..row_size].as_chunks::<4>().0)
        {
            *out = [pixel[2], pixel[1], pixel[0], 255];
        }
    }
    RgbaImage::from_raw(width, height, pixels).ok_or(CaptureError::InvalidSize)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_stride_padding_and_converts_bgra() {
        let image = bgra_rows_to_rgba(
            &[1, 2, 3, 0, 99, 99, 99, 99, 4, 5, 6, 0, 88, 88, 88, 88],
            8,
            1,
            2,
        )
        .unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [3, 2, 1, 255]);
        assert_eq!(image.get_pixel(0, 1).0, [6, 5, 4, 255]);
    }
    #[test]
    fn accepts_gpu_buffer_whose_last_row_has_no_pitch_padding() {
        // Mapped memory may end right after the last visible row.
        let image = bgra_rows_to_rgba(&[1, 2, 3, 0, 99, 99, 99, 99, 4, 5, 6, 0], 8, 1, 2).unwrap();
        assert_eq!(image.get_pixel(0, 1).0, [6, 5, 4, 255]);
    }
    #[test]
    fn finished_worker_reports_fatal_error_instead_of_waiting_for_timeout() {
        let mut latest = Latest::default();
        assert!(matches!(
            finished_worker_error(&latest),
            CaptureError::Backend(_)
        ));
        latest.error = Some(CaptureError::Closed);
        assert_eq!(finished_worker_error(&latest), CaptureError::Closed);
        latest.error = Some(CaptureError::Hidden);
        assert!(matches!(
            finished_worker_error(&latest),
            CaptureError::Backend(_)
        ));
    }
    #[test]
    fn rejects_incomplete_gpu_buffer() {
        // Shorter than one visible row, or missing part of the last visible row.
        assert_eq!(
            bgra_rows_to_rgba(&[0; 3], 8, 1, 1).unwrap_err(),
            CaptureError::InvalidSize
        );
        assert_eq!(
            bgra_rows_to_rgba(&[0; 11], 8, 1, 2).unwrap_err(),
            CaptureError::InvalidSize
        );
        assert_eq!(
            bgra_rows_to_rgba(&[0; 16], 4, 2, 2).unwrap_err(),
            CaptureError::InvalidSize
        );
    }
    #[test]
    fn uses_frame_source_age_instead_of_poll_time() {
        assert_eq!(
            source_age(12_500, 1_000, 120_000_000).unwrap(),
            Duration::from_millis(500)
        );
    }
    #[test]
    fn rejects_future_or_invalid_timestamps() {
        assert!(source_age(100, 0, 0).is_err());
        assert!(source_age(100, 1_000, 2_000_000).is_err());
    }
    #[test]
    fn future_frame_is_rejected_until_qpc_reaches_its_original_source_time() {
        let timestamp = 1_400_000;
        assert_eq!(
            source_age(1_000_000, 10_000_000, timestamp),
            Err(CaptureError::FutureFrame {
                qpc_now: 1_000_000,
                qpc_frequency: 10_000_000,
                source_timestamp_100ns: timestamp,
                delta_100ns: -400_000,
            })
        );
        // Even one future tick is rejected; the previous 10ms clamp is gone.
        assert!(matches!(
            source_age(1_399_999, 10_000_000, timestamp),
            Err(CaptureError::FutureFrame {
                delta_100ns: -1,
                ..
            })
        ));
        assert_eq!(
            source_age(1_400_000, 10_000_000, timestamp),
            Ok(Duration::ZERO)
        );
        assert_eq!(
            source_age(1_400_005, 10_000_000, timestamp),
            Ok(Duration::from_nanos(500))
        );
    }
    #[test]
    fn future_deferral_has_bounded_offset_and_wall_time() {
        assert!(can_defer_future(-400_000, Duration::from_millis(10)));
        assert!(can_defer_future(-2_000_000, Duration::ZERO));
        assert!(!can_defer_future(-2_000_001, Duration::ZERO));
        assert!(!can_defer_future(-1, MAX_FUTURE_DEFER));
        assert!(!can_defer_future(0, Duration::ZERO));
    }
    #[test]
    fn rejects_crop_outside_window_instead_of_clamping() {
        let crop = CropRect {
            x: 1,
            y: 30,
            width: 100,
            height: 80,
        };
        assert_eq!(crop.validate(100, 110), Err(CaptureError::GeometryChanged));
        assert!(crop.validate(101, 110).is_ok());
    }
    #[test]
    fn session_handle_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<WindowCaptureSession>();
    }
    fn sample_latest(source_time: Instant) -> Latest {
        Latest {
            frame: Some((
                CapturedFrame {
                    image: RgbaImage::new(5, 4),
                    backend: CaptureBackend::WindowsGraphicsCapture,
                    captured_at_ms: 100,
                    source_timestamp_100ns: Some(50),
                    sequence: 7,
                },
                source_time,
            )),
            ..Default::default()
        }
    }
    #[test]
    fn drops_stale_image_instead_of_restamping_it() {
        let now = Instant::now();
        let mut latest = sample_latest(now - Duration::from_secs(2));
        assert!(take_fresh_frame(&mut latest, 0, (5, 4), now).is_none());
        assert!(latest.frame.is_none());
    }
    #[test]
    fn never_returns_duplicate_or_old_geometry() {
        let now = Instant::now();
        assert!(take_fresh_frame(&mut sample_latest(now), 7, (5, 4), now).is_none());
        assert!(take_fresh_frame(&mut sample_latest(now), 0, (6, 4), now).is_none());
        let frame = take_fresh_frame(&mut sample_latest(now), 0, (5, 4), now).unwrap();
        assert_eq!(frame.captured_at_ms, 100);
    }
    #[test]
    fn bounds_copy_interval_for_zero_or_huge_requests() {
        assert_eq!(interval_millis(Duration::ZERO), 16);
        assert_eq!(interval_millis(Duration::from_secs(60)), 10_000);
        assert_eq!(interval_millis(DEFAULT_COPY_INTERVAL), 333);
    }
}
