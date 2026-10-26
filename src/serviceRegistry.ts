import type { ServiceId } from './types';
export const services: Record<
  ServiceId,
  { name: string; category: 'runtime' | 'web' | 'database' | 'tools'; mark: string; color: string }
> = {
  php: { name: 'PHP', category: 'runtime', mark: 'php', color: '#777bb3' },
  apache: { name: 'Apache', category: 'web', mark: 'A', color: '#c64b71' },
  mysql: { name: 'MySQL', category: 'database', mark: 'my', color: '#2785aa' },
  postgresql: { name: 'PostgreSQL', category: 'database', mark: 'P', color: '#416b9b' },
  nginx: { name: 'Nginx', category: 'web', mark: 'N', color: '#269b58' },
  redis: { name: 'Redis', category: 'database', mark: 'R', color: '#d3554f' },
  mailpit: { name: 'Mailpit', category: 'tools', mark: 'M', color: '#b98232' },
  caddy: { name: 'Caddy', category: 'tools', mark: 'C', color: '#378b91' },
};
export const defaultPorts: Record<ServiceId, number[]> = {
  php: [9000],
  apache: [8080],
  mysql: [3306],
  postgresql: [5432],
  nginx: [8081],
  redis: [6379],
  mailpit: [1025, 8025],
  caddy: [80, 443, 2019],
};
