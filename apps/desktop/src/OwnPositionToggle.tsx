import type { OwnPositionReading } from './useOwnPosition';

const statusLabel: Record<OwnPositionReading['status'], string> = {
  ready: 'potwierdzona kompasem', unconfirmed: 'niepotwierdzona', ambiguous: 'niejednoznaczna', notFound: 'brak strzałki', blocked: 'brak minimapy',
};

type Props = { enabled: boolean; setEnabled: (enabled: boolean) => void; reading: OwnPositionReading | null; message: string };

/** "Automatyczna własna pozycja (beta)" — off by default, not persisted. */
export function OwnPositionToggle({ enabled, setEnabled, reading, message }: Props) {
  const marker = reading?.marker ?? null;
  return <section className="panel">
    <label className="check-field"><input type="checkbox" checked={enabled} onChange={e => setEnabled(e.target.checked)} />
      Automatyczna własna pozycja z minimapy (beta) — ustawia punkt działa jak Ctrl+Shift+G</label>
    {enabled && <p className="hint" role="status">
      {marker ? `Strzałka gracza: ${marker.sector}, kierunek ${Math.round(marker.headingDeg)}°${marker.compassDeg !== null ? `, kompas ${Math.round(marker.compassDeg)}°` : ''}, pewność ${Math.round(marker.confidence * 100)}% (${statusLabel[reading!.status]}). ` : reading ? `Status: ${statusLabel[reading.status]}. ` : ''}
      {message}
    </p>}
    <p className="hint">Wykrywa białą strzałkę gracza na minimapie lub mapie M i sprawdza jej kierunek z kompasem (±20°). Punkt jest ustawiany dopiero po dwóch zgodnych klatkach. Gdy kamera jest obrócona względem postaci lub strzałkę zasłania ikona pojazdu, pozycja nie jest zmieniana.</p>
  </section>;
}
