import { invoke, isTauri } from '@tauri-apps/api/core';
import builtin from '../../../data/weapons/builtin.json';
import type { Profile } from './types';
export const native = isTauri();
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (native) return invoke<T>(command, args);
  if (command === 'profiles') { const {default:community}=await import('../../../data/weapons/community-profiles.json'); return [...builtin,...community] as T; }
  if (command === 'community_maps') { const {default:maps}=await import('../../../data/maps/community-maps.json'); return maps as T; }
  if (command === 'load_settings') return null as T;
  if (command === 'save_settings' || command === 'publish') return undefined as T;
  throw new Error('Ta funkcja wymaga aplikacji Windows .exe. Przeglądarka pokazuje interfejs.');
}
export async function pickImage(): Promise<string | null> {
  if (!native) throw new Error('Import obrazu wymaga wersji .exe');
  const { open } = await import('@tauri-apps/plugin-dialog');
  return await open({ multiple: false, filters: [{ name: 'Obrazy mapy lub HUD', extensions: ['png', 'jpg', 'jpeg'] }] }) as string | null;
}
export async function pickJson(): Promise<string | null> {
  if (!native) throw new Error('Import wymaga wersji .exe');
  const { open } = await import('@tauri-apps/plugin-dialog');
  return await open({ multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] }) as string | null;
}
export async function saveJson(filename: string, value: unknown): Promise<void> {
  if (!native) throw new Error('Eksport wymaga wersji .exe');
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({ defaultPath: filename, filters: [{ name: 'JSON', extensions: ['json'] }] });
  if (path) await call('export_json', { path, value });
}
export function profileFor(profiles: Profile[], selected: string, overrides: Record<string, Profile>): Profile | undefined {
  return overrides[selected] ?? profiles.find(p => p.id === selected);
}

