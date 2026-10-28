import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  Settings,
  Snapshot,
  SiteConfig,
  PathStatus,
  ServiceId,
  PhpExtensions,
  EnvironmentFolder,
  EnvironmentMove,
} from './types';
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
          {
            activeVersion: null,
            installations: [],
            ports: defaultPorts[id],
            autoStart: false,
            addToPath: false,
          },
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

export const siteConfigApi = {
  read: (id: string) => call<SiteConfig>('read_site_config', { id }),
  preview: (id: string, content: string | null) =>
    call<SiteConfig>('preview_site_config', { id, content }),
  save: (id: string, content: string | null) => call('save_site_config', { id, content }),
};

export const userPathApi = {
  status: (id: ServiceId) => call<PathStatus>('get_path_status', { id }),
  set: (id: ServiceId, enabled: boolean) => call('set_user_path', { id, enabled }),
  install: (id: ServiceId, version: string, addToPath: boolean) =>
    call('install_package', { id, version, addToPath }),
};
export const phpExtensionsApi = {
  list: () => call<PhpExtensions>('get_php_extensions'),
  set: (version: string, extension: string, enabled: boolean) =>
    call<PhpExtensions>('set_php_extension', { version, extension, enabled }),
  cancel: () => call('cancel_install', { id: 'php' }),
};

export const environmentApi = {
  choose: () => call<EnvironmentFolder | null>('choose_environment_folder'),
  move: (token: string) => call<EnvironmentMove>('move_environment', { token }),
  cleanup: () => call<EnvironmentMove>('cleanup_environment'),
};
