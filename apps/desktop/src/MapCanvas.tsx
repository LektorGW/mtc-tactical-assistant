import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from 'react';
import { Maximize2, ZoomIn, ZoomOut } from 'lucide-react';
import { clampToImage, clientToImage, columnLabels, fitView, formatDistance, localBearing, panView, rowLabels, zoomLevel, zoomView, type View } from './map-geometry';
import type { MapGrid, Point } from './types';

export type MapMarker = { key: string; point: Point; label: string; color: string; draggable?: boolean };
/** Salvo forecast already converted to image pixels (see `salvo.ts` `ellipseOnMap`). */
export type MapSalvoLayer = {
  ellipses: { key: string; cx: number; cy: number; rx: number; ry: number; rotationDeg: number; color: string; dashed?: boolean }[];
  center: Point;
  label: string;
};
type Props = {
  image: string | null;
  width: number;
  height: number;
  grid: MapGrid;
  markers: MapMarker[];
  /** Solid line between the two points (e.g. gun → target) with a distance label. */
  line?: { from: Point; to: Point; color: string } | null;
  secondaryLine?: { from: Point; to: Point; color: string } | null;
  scale: number | null;
  rotation: number;
  unit: string;
  dispersionRadius?: number | null;
  salvo?: MapSalvoLayer | null;
  placeholder?: string;
  /** Left click (no drag) places the active tool; right click places the target. */
  onPlace: (point: Point, button: 'primary' | 'secondary') => void;
  onMove: (key: string, point: Point) => void;
};
type Gesture = { kind: 'pan' | 'drag'; key?: string; pointerId: number; startX: number; startY: number; view: View; moved: boolean };

/**
 * Interactive SVG map in image-pixel coordinates (viewBox = visible part of the
 * natural image). Pointer positions are converted with the element box, so DPI
 * scaling and the displayed size never leak into map coordinates.
 */
export function MapCanvas({ image, width, height, grid, markers, line, secondaryLine, scale, rotation, unit, dispersionRadius, salvo, placeholder, onPlace, onMove }: Props) {
  const svg = useRef<SVGSVGElement>(null);
  const [view, setView] = useState<View>(() => fitView(width, height));
  const viewRef = useRef(view); viewRef.current = view;
  const gesture = useRef<Gesture | null>(null);
  const [boxWidth, setBoxWidth] = useState(800);

  useEffect(() => { setView(fitView(width, height)); }, [width, height, image]);
  useEffect(() => {
    const element = svg.current; if (!element) return;
    const observer = new ResizeObserver(entries => { const w = entries[0]?.contentRect.width; if (w) setBoxWidth(w); });
    observer.observe(element);
    // React registers wheel listeners as passive; zoom needs preventDefault.
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const box = element.getBoundingClientRect();
      const factor = Math.exp(-event.deltaY * 0.0015);
      // Anchor from the latest queued view: several wheel events can arrive before a render.
      setView(current => zoomView(current, factor, clientToImage(event.clientX, event.clientY, box, current), width, height));
    };
    element.addEventListener('wheel', wheel, { passive: false });
    return () => { observer.disconnect(); element.removeEventListener('wheel', wheel); };
  }, [width, height]);

  const toImage = (event: { clientX: number; clientY: number }) => clientToImage(event.clientX, event.clientY, svg.current!.getBoundingClientRect(), viewRef.current);
  const k = view.width / Math.max(1, boxWidth); // image px per CSS px: keeps markers/text constant on screen.

  const down = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (event.button === 2) { event.preventDefault(); onPlace(clampToImage(toImage(event), width, height), 'secondary'); return; }
    if (event.button !== 0 && event.button !== 1) return;
    const key = event.button === 0 ? (event.target as Element).closest('[data-marker]')?.getAttribute('data-marker') ?? undefined : undefined;
    gesture.current = { kind: key ? 'drag' : 'pan', key, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, view: viewRef.current, moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const move = (event: ReactPointerEvent<SVGSVGElement>) => {
    const g = gesture.current; if (!g || g.pointerId !== event.pointerId) return;
    if (!g.moved && Math.hypot(event.clientX - g.startX, event.clientY - g.startY) < 4) return;
    g.moved = true;
    if (g.kind === 'drag' && g.key) onMove(g.key, clampToImage(toImage(event), width, height));
    else {
      const box = svg.current!.getBoundingClientRect();
      const s = Math.min(box.width / g.view.width, box.height / g.view.height);
      setView(panView(g.view, -(event.clientX - g.startX) / s, -(event.clientY - g.startY) / s, width, height));
    }
  };
  const up = (event: ReactPointerEvent<SVGSVGElement>) => {
    const g = gesture.current; if (!g || g.pointerId !== event.pointerId) return;
    gesture.current = null;
    if (!g.moved && event.button === 0) onPlace(clampToImage(toImage(event), width, height), 'primary');
  };
  const zoomBy = (factor: number) => setView(current => zoomView(current, factor, { x: current.x + current.width / 2, y: current.y + current.height / 2 }, width, height));
  const key = (event: KeyboardEvent<SVGSVGElement>) => {
    const step = view.width * 0.15;
    const moves: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    if (event.key === '+' || event.key === '=') zoomBy(1.4);
    else if (event.key === '-') zoomBy(1 / 1.4);
    else if (event.key === '0') setView(fitView(width, height));
    else if (moves[event.key]) setView(current => panView(current, moves[event.key][0], moves[event.key][1], width, height));
    else return;
    event.preventDefault();
  };

  const n = grid.divisions;
  const cols = columnLabels(n, grid.columns);
  const rows = rowLabels(n, grid.rowsFrom);
  const cellW = width / n, cellH = height / n;
  const bearing = line && scale ? localBearing(line.from, line.to, scale, rotation) : null;
  const font = 12 * k;
  return <div className="map-canvas">
    <div className="map-canvas-tools" role="toolbar" aria-label="Widok mapy">
      <button type="button" onClick={() => zoomBy(1.4)} aria-label="Przybliż"><ZoomIn size={14} /></button>
      <button type="button" onClick={() => zoomBy(1 / 1.4)} aria-label="Oddal"><ZoomOut size={14} /></button>
      <button type="button" onClick={() => setView(fitView(width, height))} aria-label="Cała mapa"><Maximize2 size={14} /></button>
      <span>{zoomLevel(view, width).toFixed(1)}×</span>
    </div>
    <svg ref={svg} className="tactical-map" tabIndex={0} viewBox={`${view.x} ${view.y} ${view.width} ${view.height}`}
      style={{ aspectRatio: `${width} / ${height}`, width: `min(100%, calc(70vh * ${width / height}))` }}
      onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={() => { gesture.current = null; }}
      onContextMenu={event => event.preventDefault()} onKeyDown={key}
      role="application" aria-label="Mapa: lewy klik — aktywne narzędzie, prawy klik — cel, przeciąganie — przesuwanie lub punkt, kółko — zoom, strzałki i +/− z klawiatury">
      <rect x={0} y={0} width={width} height={height} fill="#101b25" />
      {image && <image href={image} x={0} y={0} width={width} height={height} preserveAspectRatio="none" />}
      {grid.visible && <g className="map-grid" pointerEvents="none">
        {Array.from({ length: n - 1 }, (_, i) => <line key={`v${i}`} x1={(i + 1) * cellW} y1={0} x2={(i + 1) * cellW} y2={height} stroke="#e8eef7" strokeOpacity={0.35} strokeWidth={k} />)}
        {Array.from({ length: n - 1 }, (_, i) => <line key={`h${i}`} x1={0} y1={(i + 1) * cellH} x2={width} y2={(i + 1) * cellH} stroke="#e8eef7" strokeOpacity={0.35} strokeWidth={k} />)}
        {cols.map((label, i) => <text key={`c${i}`} x={(i + 0.5) * cellW} y={view.y + 15 * k} textAnchor="middle" fontSize={font} className="map-grid-label">{label}</text>)}
        {rows.map((label, i) => <text key={`r${i}`} x={view.x + 6 * k} y={(i + 0.5) * cellH + 4 * k} fontSize={font} className="map-grid-label">{label}</text>)}
      </g>}
      {secondaryLine && <line x1={secondaryLine.from.x} y1={secondaryLine.from.y} x2={secondaryLine.to.x} y2={secondaryLine.to.y} stroke={secondaryLine.color} strokeWidth={2 * k} pointerEvents="none" />}
      {line && <g pointerEvents="none">
        <line x1={line.from.x} y1={line.from.y} x2={line.to.x} y2={line.to.y} stroke="#0b1119" strokeOpacity={0.7} strokeWidth={5 * k} />
        <line x1={line.from.x} y1={line.from.y} x2={line.to.x} y2={line.to.y} stroke={line.color} strokeWidth={2.5 * k} strokeDasharray={`${8 * k} ${5 * k}`} />
        {bearing && <text x={(line.from.x + line.to.x) / 2} y={(line.from.y + line.to.y) / 2 - 8 * k} textAnchor="middle" fontSize={13 * k} className="map-line-label">{formatDistance(bearing.distance, unit)} · {bearing.azimuthDeg.toFixed(1)}°</text>}
      </g>}
      {dispersionRadius && scale && line && <circle cx={line.to.x} cy={line.to.y} r={dispersionRadius / scale} fill="#e6af461a" stroke="#e6af46" strokeWidth={k} pointerEvents="none" />}
      {salvo && <g className="map-salvo" pointerEvents="none">
        {salvo.ellipses.map(e => <ellipse key={e.key} cx={e.cx} cy={e.cy} rx={Math.max(e.rx, k)} ry={Math.max(e.ry, k)} transform={`rotate(${e.rotationDeg} ${e.cx} ${e.cy})`}
          fill={e.color} fillOpacity={0.08} stroke={e.color} strokeWidth={1.5 * k} strokeDasharray={e.dashed ? `${5 * k} ${4 * k}` : undefined} />)}
        <path d={`M ${salvo.center.x - 6 * k} ${salvo.center.y} L ${salvo.center.x + 6 * k} ${salvo.center.y} M ${salvo.center.x} ${salvo.center.y - 6 * k} L ${salvo.center.x} ${salvo.center.y + 6 * k}`} stroke="#ff7f6e" strokeWidth={2 * k} />
        <text x={salvo.center.x + 10 * k} y={salvo.center.y + 18 * k} fontSize={11 * k} fill="#ffb3a8" className="map-marker-label">{salvo.label}</text>
      </g>}
      {markers.map(m => <g key={m.key} data-marker={m.draggable ? m.key : undefined} className={m.draggable ? 'map-marker draggable' : 'map-marker'}>
        <circle cx={m.point.x} cy={m.point.y} r={11 * k} fill={m.color} fillOpacity={0.2} stroke={m.color} strokeWidth={2 * k} />
        <circle cx={m.point.x} cy={m.point.y} r={3 * k} fill={m.color} />
        <text x={m.point.x + 14 * k} y={m.point.y - 10 * k} fontSize={font} fill={m.color} className="map-marker-label">{m.label}</text>
      </g>)}
      {!image && placeholder && <text x={view.x + view.width / 2} y={view.y + view.height / 2} textAnchor="middle" fill="#718299" fontSize={16 * k}>{placeholder}</text>}
      <g transform={`translate(${view.x + view.width - 26 * k},${view.y + 52 * k}) rotate(${-rotation})`} pointerEvents="none">
        <path d={`M 0 ${-16 * k} L ${-7 * k} ${10 * k} L 0 ${4 * k} L ${7 * k} ${10 * k} Z`} fill="#d9e2ef" stroke="#0b1119" strokeWidth={k} />
        <text y={-19 * k} textAnchor="middle" fontSize={10 * k} fill="#d9e2ef">N</text>
      </g>
    </svg>
  </div>;
}
