import { useRef, useState, type PointerEvent } from 'react';
import type { Roi } from './types';

export function RoiEditor({ image, selected, regions, onChange }: { image: string; selected: string; regions: { id: string; label: string; roi: Roi; color: string }[]; onChange: (roi: Roi) => void }) {
  const start = useRef<{ x: number; y: number } | null>(null);
  const [draft, setDraft] = useState<Roi | null>(null);
  const point = (event: PointerEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return { x: Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)), y: Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height)) };
  };
  const finish = () => { if (draft && draft.width > 0.002 && draft.height > 0.002) onChange(draft); start.current = null; setDraft(null); };
  return <div className="roi-editor"><img src={image} alt="Wybrana klatka — przeciągnij prostokąt obszaru HUD; poniżej dostępne są też pola liczbowe" /><svg viewBox="0 0 1000 1000" preserveAspectRatio="none" aria-label="Zaznaczanie obszaru analizy" onPointerDown={e => { e.currentTarget.setPointerCapture(e.pointerId); start.current = point(e); setDraft(null); }} onPointerMove={e => { if (!start.current) return; const p = point(e); const a = start.current; setDraft({ x: Math.min(a.x, p.x), y: Math.min(a.y, p.y), width: Math.abs(a.x - p.x), height: Math.abs(a.y - p.y) }); }} onPointerUp={finish} onPointerCancel={() => { start.current = null; setDraft(null); }}>
    {regions.map(r => <g key={r.id}><rect x={r.roi.x * 1000} y={r.roi.y * 1000} width={r.roi.width * 1000} height={r.roi.height * 1000} fill={r.color} fillOpacity={selected === r.id ? 0.16 : 0.05} stroke={r.color} strokeWidth={selected === r.id ? 3 : 1.5} vectorEffect="non-scaling-stroke" /><text x={r.roi.x * 1000 + 5} y={Math.max(15, r.roi.y * 1000 - 7)} fontSize="19" fill={r.color}>{r.label}</text></g>)}
    {draft && <rect x={draft.x * 1000} y={draft.y * 1000} width={draft.width * 1000} height={draft.height * 1000} fill="#fff" fillOpacity="0.1" stroke="#fff" strokeWidth="2" vectorEffect="non-scaling-stroke" />}
  </svg></div>;
}
