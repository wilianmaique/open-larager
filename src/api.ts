import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Settings, Snapshot } from './types';
import { serviceIds } from './types';
import { defaultPorts } from './serviceRegistry';
import catalog from '../src-tauri/catalog/packages.json';

export const native = isTauri();
export const packages = catalog.packages;
export function emptySnapshot(): Snapshot {
  return {
    root: '',
    systemLocale: navigator.language,
    hostsSynced: true,
    busyService: null,
    certificate: { available: false, trusted: false, fingerprint: null },
    services: serviceIds.map((id) => ({
      id,
      status: 'notInstalled',
      version: null,
      pid: null,
      uptime: 0,
      message: null,
    })),
    settings: {
      schemaVersion: 1,
      language: 'auto',
      theme: 'system',
      sites: [],
      services: Object.fromEntries(
        serviceIds.map((id) => [
          id,
          { activeVersion: null, installations: [], ports: defaultPorts[id], autoStart: false },
        ]),
      ) as unknown as Settings['services'],
    },
  };
}
export async function call<T = void>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!native) throw new Error('DESKTOP_REQUIRED');
  return invoke<T>(command, args);
}
export const getSnapshot = () =>
  native ? call<Snapshot>('get_snapshot') : Promise.resolve(emptySnapshot());
