/** Mirrors `mtc_vision::LoadoutSpecs` returned by `read_loadout_specs` / `read_loadout_specs_file`. */
export type SpecValue = { value: number; text: string };
export type LoadoutSpecs = {
  schemaVersion: number;
  status: 'ready' | 'partial' | 'blocked';
  timestampMs: number;
  /** "Primary" | "Secondary" | "Tertiary"… from the "Choose Weapon - …" title. */
  slot: string | null;
  /** Always null: the Specifications panel does not name the weapon. */
  weaponName: string | null;
  damage: SpecValue | null;
  projectileType: string | null;
  muzzleVelocityMps: SpecValue | null;
  penetrationMm: SpecValue | null;
  projectileMassKg: SpecValue | null;
  /** Heuristic 0–0.7 (share of parsed fields), not a probability. */
  confidence: number;
  /** "Odczytane z UI gry (…)" */
  source: string;
  lines: string[];
  warnings: string[];
};
