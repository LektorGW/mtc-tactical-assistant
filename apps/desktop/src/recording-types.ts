import type { VisionConfig } from './types';

export type RecordingFormat = 'png' | 'jpeg';
export type RecordingBackend = 'wgc' | 'gdi';

/** Values edited in the panel. Consent is deliberately not part of this object. */
export type RecordingOptions = {
  format: RecordingFormat;
  jpegQuality: number;
  maxFps: number;
  maxFrames: number;
  maxMegabytes: number;
  maxSeconds: number;
};

export type RecordingRequest = RecordingOptions & {
  consent: true;
  directory: string;
  handle: number;
  backend: RecordingBackend;
  config: VisionConfig | null;
};

export type RecordingStatus = {
  active: boolean;
  directory: string | null;
  handle: number | null;
  backend: RecordingBackend | null;
  format: RecordingFormat | null;
  frames: number;
  bytes: number;
  skipped: number;
  startedAtMs: number | null;
  elapsedMs: number;
  lastFrameMs: number | null;
  maxFrames: number;
  maxBytes: number;
  maxDurationMs: number;
  stopReason: string | null;
  warning: string | null;
  error: string | null;
};

export type RecordedFrame = { index: number; file: string; timestampMs: number; width: number; height: number };

export type LoadedRecording = {
  directory: string;
  session: {
    schemaVersion: number;
    createdAtMs: number;
    endedAtMs: number | null;
    completed: boolean;
    stopReason: string | null;
    format: RecordingFormat;
    source: { handle: number | null; windowTitle: string | null; backend: string };
    analysisConfig: VisionConfig | null;
    totalBytes: number;
  };
  frames: (RecordedFrame & { path: string })[];
};

export type RecordingRegionSummary = {
  id: string; kind: string; frames: number; observed: number; stable: number;
  ambiguous: number; empty: number; errors: number; blocked: number;
  lastText: string | null; lastValue: number | null;
};

export type RecordingAnalysis = {
  directory: string;
  totalFrames: number;
  start: number;
  frames: { index: number; file: string; timestampMs: number; error: string | null;
    analysis: { status: string; ocrRegions: { id: string; status: string; text: string; observation: { value: number; confidence: number } | null }[] } | null }[];
  regions: RecordingRegionSummary[];
  warnings: string[];
  elapsedMs: number;
};
