export const serviceIds = [
  'php',
  'apache',
  'mysql',
  'postgresql',
  'nginx',
  'redis',
  'mailpit',
  'caddy',
] as const;
export type ServiceId = (typeof serviceIds)[number];
export type Page = 'overview' | 'services' | 'domains' | 'logs' | 'settings';
export type Locale = 'en' | 'pt-BR' | 'es' | 'zh-CN' | 'hi';
export interface Package {
  service: ServiceId;
  version: string;
  url: string;
  sha256: string;
  archiveRoot: string;
  source: string;
  sourceUrl: string;
  size: number;
  community: boolean;
  verification: string;
}
export interface ServiceSettings {
  activeVersion: string | null;
  installations: { version: string; installedAt: number; sha256: string }[];
  ports: number[];
  autoStart: boolean;
}
export interface Site {
  id: string;
  hostname: string;
  root: string;
  mode: 'php' | 'static' | 'proxy';
  upstreamPort: number | null;
  enabled: boolean;
}
export interface Settings {
  schemaVersion: number;
  language: string;
  theme: string;
  services: Record<ServiceId, ServiceSettings>;
  sites: Site[];
}
export interface ServiceStatus {
  id: ServiceId;
  status: 'running' | 'stopped' | 'notInstalled' | 'error';
  version: string | null;
  pid: number | null;
  uptime: number;
  message: string | null;
}
export interface Snapshot {
  root: string;
  systemLocale: string;
  settings: Settings;
  services: ServiceStatus[];
  hostsSynced: boolean;
  busyService: ServiceId | null;
  certificate: { available: boolean; trusted: boolean; fingerprint: string | null };
}
export interface Progress {
  service: ServiceId;
  version: string;
  phase: 'downloading' | 'verifying' | 'extracting' | 'complete';
  downloaded: number;
  total: number | null;
}
export interface Credentials {
  username: string;
  password: string;
}
