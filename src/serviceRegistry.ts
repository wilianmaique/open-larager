import registry from '../src-tauri/catalog/services.json' with { type: 'json' };
import type { ServiceId } from './types';

// Rust and React read the same compiled capabilities and default ports.
export const services = registry;
export const defaultPorts = Object.fromEntries(
  Object.entries(registry).map(([id, definition]) => [id, definition.ports]),
) as Record<ServiceId, number[]>;
