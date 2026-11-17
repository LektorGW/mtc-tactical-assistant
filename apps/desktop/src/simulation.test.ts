import { describe, expect, it } from 'vitest';
import { endSimulation, simulationActive, SIMULATION_MAP_NAME } from './simulation';
import { defaultSettings, type Profile, type Settings } from './types';

const profile = (id: string, source: string): Profile => ({ schemaVersion: 1, id, name: id, category: 'tube_artillery', gameVersion: 'x', status: 'experimental', source, calibrated: true, speed: 100, gravity: 10, minElevationDeg: -5, maxElevationDeg: 80, dispersionSigma: null, salvoSize: 1, notes: '' });
const simulated: Settings = { ...defaultSettings, selected: 'tube', mapPath: null, mapName: SIMULATION_MAP_NAME, scale: 1, uncertainty: 2, own: { x: 180, y: 430 }, target: { x: 610, y: 170 },
  overrides: { tube: profile('tube', 'SYMULACJA: v=100'), real: profile('real', 'Pomiar w grze') } };

describe('tryb symulacji', () => {
  it('jest aktywny przy syntetycznej mapie lub profilu demo', () => {
    expect(simulationActive(simulated)).toBe(true);
    expect(simulationActive({ ...simulated, mapName: 'Rohkstov', mapPath: 'C:/x.png' })).toBe(true);
    expect(simulationActive({ ...simulated, mapName: 'Rohkstov', mapPath: 'C:/x.png', selected: 'real' })).toBe(false);
  });
  it('zakończenie usuwa tylko dane symulacji', () => {
    const ended = endSimulation(simulated);
    expect(simulationActive(ended)).toBe(false);
    expect(ended.overrides.tube).toBeUndefined();
    expect(ended.overrides.real).toBeDefined();
    expect([ended.own, ended.target, ended.scale]).toEqual([null, null, null]);
    // A real map keeps its points and scale; only the demo profile goes away.
    const real = endSimulation({ ...simulated, mapName: 'Rohkstov', mapPath: 'C:/x.png' });
    expect(real.own).toEqual(simulated.own);
    expect(real.scale).toBe(1);
    expect(real.overrides.tube).toBeUndefined();
    expect(endSimulation(real)).toBe(real);
  });
});
