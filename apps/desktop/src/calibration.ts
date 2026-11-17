import type { Profile, Unit } from './types';
export type CalibrationSample = { range: number; elevationDeg: number; flightTime: number; heightDelta: number; unit: Unit; source: string };
export type CalibrationFit = { fittedProfile: Profile; rmsResidualUnits: number; maxResidualUnits: number; maxRelativeResidual: number; conditionNumber: number; observedRangeMin: number; observedRangeMax: number; assumptions: string[]; residuals: {sampleIndex:number;residualUnits:number}[] };
export function parseCalibrationCsv(text: string, unit: Unit, source: string): CalibrationSample[] {
  if(!source.trim())throw new Error('Podaj źródło pomiarów');
  const lines=text.trim().split(/\r?\n/).filter(line=>line.trim());
  if(lines.length<3||lines.length>100)throw new Error('Wymagane 3–100 pomiarów');
  return lines.map((line,i)=>{
    const fields=line.split(';').map(v=>v.trim());
    if(fields.length!==4||fields.some(v=>!v))throw new Error(`Wiersz ${i+1}: wymagane cztery liczby oddzielone średnikiem`);
    const [range,elevationDeg,flightTime,heightDelta]=fields.map(Number);
    if([range,elevationDeg,flightTime,heightDelta].some(v=>!Number.isFinite(v))||range<=0||flightTime<=0||elevationDeg<=-89||elevationDeg>=89)throw new Error(`Wiersz ${i+1}: niepoprawne wartości pomiaru`);
    return {range,elevationDeg,flightTime,heightDelta,unit,source:source.trim()};
  });
}
