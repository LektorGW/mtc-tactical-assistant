import { describe,it,expect } from 'vitest';
import { parseCalibrationCsv } from './calibration';
describe('pomiary kalibracyjne',()=>{
  it('zachowuje wysokość, jednostki i źródło każdej próbki',()=>{
    const values=parseCalibrationCsv('100;10;1;-2\n200;20;2;0\n300;30;3;10','game_meters','pomiar w grze');
    expect(values[0].heightDelta).toBe(-2);expect(values[2].unit).toBe('game_meters');expect(values.every(v=>v.source==='pomiar w grze')).toBe(true);
  });
  it('odrzuca pustą wysokość, Infinity i brak źródła',()=>{
    for(const text of ['100;10;1;\n200;20;2;0\n300;30;3;10','100;10;Infinity;0\n200;20;2;0\n300;30;3;10'])expect(()=>parseCalibrationCsv(text,'game_units','test')).toThrow();
    expect(()=>parseCalibrationCsv('100;10;1;0\n200;20;2;0\n300;30;3;10','game_units','')).toThrow();
  });
});
