import { defaultSettings, type Profile, type Settings } from './types';

/** Map name and profile-source marker written by the built-in simulation. */
export const SIMULATION_MAP_NAME = 'Siatka treningowa — dane syntetyczne';
export const SIMULATION_SOURCE_PREFIX = 'SYMULACJA';

export function isSimulationProfile(profile: Pick<Profile, 'source'> | undefined | null): boolean {
  return !!profile && profile.source.startsWith(SIMULATION_SOURCE_PREFIX);
}

/** Simulation is active while its synthetic map or its synthetic profile is in use. */
export function simulationActive(s: Settings): boolean {
  return (s.mapPath === null && s.mapName === SIMULATION_MAP_NAME) || isSimulationProfile(s.overrides[s.selected]);
}

/**
 * Leaves the simulation: removes only data created by it (synthetic map, its
 * points and scale, the demo profile override). Real maps and profiles stay.
 */
export function endSimulation(s: Settings): Settings {
  const overrides = Object.fromEntries(Object.entries(s.overrides).filter(([, profile]) => !isSimulationProfile(profile)));
  const syntheticMap = s.mapPath === null && s.mapName === SIMULATION_MAP_NAME;
  if (!syntheticMap && Object.keys(overrides).length === Object.keys(s.overrides).length) return s;
  return {
    ...s,
    overrides,
    ...(syntheticMap ? { mapName: defaultSettings.mapName, own: null, target: null, scale: null, uncertainty: 0 } : {}),
  };
}
