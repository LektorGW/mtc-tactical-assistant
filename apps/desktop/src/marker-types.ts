import type { Calibration, MarkerSignature, Point, Roi, VisionConfig } from './types';

export type SelectedMarkerRole = 'own' | 'target';
export type MarkerCaptureBackend = 'wgc' | 'gdi';

export type MarkerSignatureProposal = {
  id: string;
  signature: MarkerSignature;
  framePositionNormalized: Point;
  boundingBox: Roi;
  pixelCount: number;
  aspectRatio: number;
  fillRatio: number;
  clipped: boolean;
  colorSpread: number;
  confidence: number;
};

export type MarkerSelectionHint = {
  selectionId: string;
  signatureId: string;
  role: SelectedMarkerRole;
  confirmedPoint: Point;
  timestampMs: number;
  scope: string;
  frameWidth: number;
  frameHeight: number;
  mapWidth: number;
  mapHeight: number;
};

export type NativeMarkerSelection = {
  hint: MarkerSelectionHint;
  handle: number;
  backend: MarkerCaptureBackend;
  mapId: string | null;
  mapRoi: Roi;
  calibration: Calibration | null;
  captureEpoch: number;
};

export type MarkerSetupPreview = {
  token: string;
  preview: string;
  width: number;
  height: number;
  previewWidth: number;
  previewHeight: number;
  timestampMs: number;
  mapRoi: Roi;
  mapId: string | null;
  proposals: MarkerSignatureProposal[];
  warnings: string[];
  observedMapWidth: number;
  observedMapHeight: number;
};

export type CalibratedVisibleMarker = {
  signature: MarkerSignature;
  selection: NativeMarkerSelection;
  preview: string;
  width: number;
  height: number;
  previewWidth: number;
  previewHeight: number;
  timestampMs: number;
  proposal: MarkerSignatureProposal;
};

export type MarkerPickerProps = {
  handle: number | null;
  backend: MarkerCaptureBackend;
  config: VisionConfig;
  autoMap: boolean;
  mapId: string | null;
  scope: string;
  selections: NativeMarkerSelection[];
  onSelect: (signature: MarkerSignature, selection: NativeMarkerSelection, preview: string) => void;
  notice: (text: string) => void;
};
