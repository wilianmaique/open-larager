import { useState } from 'react';
import {
  ArrowDownToLine,
  ArrowRight,
  Box,
  CircleDot,
  Code2,
  FolderOpen,
  Globe2,
  HardDrive,
  Leaf,
  LockKeyhole,
  Play,
  Plus,
  RotateCw,
  Search,
  Settings2,
  ShieldCheck,
  Square,
} from 'lucide-react';
import { Button, Empty, IconButton, ServiceIcon, Status } from './components';
import { call, packages } from './api';
import { useApp } from './state';
import { services } from './serviceRegistry';
import type { Key } from './i18n';
import type { ServiceId } from './types';

export function Overview() {
  const { t, snapshot, setPage, run, busy } = useApp();
  const running = snapshot.services.filter((s) => s.status === 'running').length;
  const installed = Object.values(snapshot.settings.services).reduce(
    (n, s) => n + s.installations.length,
    0,
  );
  return (
    <>
      <section className="hero">
        <div className="hero-copy">
          <div className="hero-eyebrow">
            <span />
            {t('workspaceReady')}
          </div>
          <h1>
            {t('greeting')}
            <br />
            <span>{t('greetingAccent')}</span>
          </h1>
          <p>{t('heroDescription')}</p>
          <div className="hero-actions">
            <Button
              variant="primary"
              disabled={busy || installed === 0}
              onClick={() =>
                void run(() => call('stack_action', { action: 'start' }), t('actionSuccess'))
              }
            >
              <Play size={15} fill="currentColor" />
              {t('startAll')}
            </Button>
            <Button
              variant="ghost"
              onClick={() => void run(() => call('open_location', { location: 'projects' }))}
            >
              <FolderOpen size={16} />
              {t('projectFolder')}
              <ArrowUpRightTiny />
            </Button>
          </div>
        </div>
        <div className="hero-art" aria-hidden="true">
          <div className="art-orbit orbit-one" />
          <div className="art-orbit orbit-two" />
          <div className="art-cross cross-one">+</div>
          <div className="art-cross cross-two">+</div>
          <div className="art-code">
            <Code2 size={30} />
          </div>
          <div className="art-window">
            <div className="art-title">
              <span />
              <span />
              <span />
              <code>localhost</code>
            </div>
            <div className="art-lines">
              <i />
              <i />
              <i />
              <i />
            </div>
            <div className="art-check">
              <ShieldCheck size={17} />
              <span>127.0.0.1</span>
              <span className="art-led" />
            </div>
          </div>
          <div className="art-lock">
            <LockKeyhole size={22} />
            <span>HTTPS</span>
          </div>
          <div className="art-leaf">
            <Leaf size={17} />
          </div>
        </div>
      </section>
      <section className="stats-grid" aria-label={t('overview')}>
        {[
          {
            icon: CircleDot,
            label: 'activeServices',
            value: running,
            hint: 'noServicesRunning',
            accent: true,
          },
          { icon: Box, label: 'installedPackages', value: installed, hint: 'packagesHint' },
          {
            icon: Globe2,
            label: 'localDomains',
            value: snapshot.settings.sites.length,
            hint: 'domainsHint',
          },
        ].map((item) => (
          <div className="stat-card" key={item.label}>
            <div className="stat-top">
              <span>{t(item.label as Key)}</span>
              <item.icon size={17} />
            </div>
            <div className={`stat-number ${item.accent && running > 0 ? 'text-green' : ''}`}>
              {String(item.value).padStart(2, '0')}
              <span className="stat-bar" />
            </div>
            <small>{t(item.hint as Key)}</small>
          </div>
        ))}
        <div className="stat-card stat-security">
          <div className="stat-top">
            <span>{t('secureByDesign')}</span>
            <ShieldCheck size={17} />
          </div>
          <div className="security-address">
            127.0.0.1
            <LockKeyhole size={15} />
          </div>
          <small>{t('loopbackHint')}</small>
        </div>
      </section>
      <ServiceList compact />
      <section className="bottom-callout">
        <span className="callout-icon">
          <Globe2 size={23} />
        </span>
        <div>
          <strong>{t('domainsTitle')}</strong>
          <p>{t('domainsDescription')}</p>
        </div>
        <Button variant="ghost" onClick={() => setPage('domains')}>
          {t('addDomain')}
          <ArrowRight size={16} />
        </Button>
      </section>
    </>
  );
}
function ArrowUpRightTiny() {
  return <span aria-hidden="true">↗</span>;
}

export function ServicesPage() {
  const { t } = useApp();
  return (
    <>
      <div className="page-heading">
        <div>
          <span className="section-eyebrow">{t('environment')}</span>
          <h1>{t('servicesTitle')}</h1>
          <p>{t('servicesDescription')}</p>
        </div>
        <span className="heading-tag">
          <HardDrive size={14} />
          {t('noPackagesBundled')}
        </span>
      </div>
      <ServiceList />
    </>
  );
}

function ServiceList({ compact = false }: { compact?: boolean }) {
  const { t, snapshot, setSelected, run, busy, progress } = useApp();
  const [filter, setFilter] = useState<'all' | 'installed'>('all');
  const [query, setQuery] = useState('');
  const statuses = snapshot.services.filter(
    (service) =>
      (filter === 'all' || service.version) &&
      services[service.id].name.toLowerCase().includes(query.toLowerCase()),
  );
  const installed = snapshot.services.filter((s) => s.version).length;
  const running = snapshot.services.some((s) => s.status === 'running');
  const action = (id: ServiceId, name: string) =>
    void run(() => call('service_action', { id, action: name }), t('actionSuccess'));
  return (
    <section className="services-section">
      <div className="section-heading">
        <div>
          <h2>
            {t('servicesTitle')}
            <span className="count-badge">{snapshot.services.length}</span>
          </h2>
          {compact && <p>{t('servicesDescription')}</p>}
        </div>
        <Button
          disabled={busy || !running}
          onClick={() =>
            void run(() => call('stack_action', { action: 'stop' }), t('actionSuccess'))
          }
        >
          <Square size={12} />
          {t('stopAll')}
        </Button>
      </div>
      <div className="service-panel">
        <div className="service-toolbar">
          <div className="filter-tabs">
            <button onClick={() => setFilter('all')} className={filter === 'all' ? 'selected' : ''}>
              {t('all')}
              <span>{snapshot.services.length}</span>
            </button>
            <button
              onClick={() => setFilter('installed')}
              className={filter === 'installed' ? 'selected' : ''}
            >
              {t('installed')}
              <span>{installed}</span>
            </button>
          </div>
          <label className="table-search">
            <Search size={15} />
            <input
              aria-label={t('search')}
              placeholder={t('search')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
        </div>
        <div className="service-table" role="table" aria-label={t('servicesTitle')}>
          <div className="table-head" role="row">
            <span role="columnheader">{t('service')}</span>
            <span role="columnheader">{t('version')}</span>
            <span role="columnheader">{t('ports')}</span>
            <span role="columnheader">{t('status')}</span>
            <span role="columnheader">{t('actions')}</span>
          </div>
          {statuses.map((status) => {
            const id = status.id;
            const service = services[id];
            const latest = packages.find((p) => p.service === id);
            const serviceProgress = progress?.service === id ? progress : null;
            const percentage = serviceProgress?.total
              ? Math.min(
                  100,
                  Math.round((serviceProgress.downloaded / serviceProgress.total) * 100),
                )
              : 0;
            return (
              <div className="service-row" role="row" key={id} data-testid={`service-${id}`}>
                <div className="service-name-cell" role="cell">
                  <ServiceIcon id={id} />
                  <button className="service-title" onClick={() => setSelected(id)}>
                    <strong>
                      {service.name}
                      {id === 'caddy' && <span className="optional">{t('optional')}</span>}
                    </strong>
                    <small>
                      {t(
                        `category${service.category.charAt(0).toUpperCase() + service.category.slice(1)}` as Key,
                      )}
                    </small>
                  </button>
                </div>
                <div className="version-cell" role="cell">
                  <code>{status.version ?? latest?.version ?? '—'}</code>
                  {!status.version && <small>{t('available')}</small>}
                </div>
                <div className="port-cell" role="cell">
                  {snapshot.settings.services[id].ports.map((port) => (
                    <span key={port}>{port}</span>
                  ))}
                </div>
                <div role="cell">
                  {serviceProgress ? (
                    <div className="row-progress">
                      <span>
                        {t(serviceProgress.phase)}{' '}
                        {serviceProgress.phase === 'downloading' && `${percentage}%`}
                      </span>
                      <progress value={percentage} max={100} />
                    </div>
                  ) : (
                    <Status status={status.status} />
                  )}
                </div>
                <div className="row-actions" role="cell">
                  {!status.version ? (
                    <Button disabled={busy} onClick={() => setSelected(id)}>
                      <Plus size={14} />
                      {t('install')}
                    </Button>
                  ) : (
                    <>
                      <IconButton
                        label={`${t('restart')} ${service.name}`}
                        disabled={busy || status.status !== 'running'}
                        onClick={() => action(id, 'restart')}
                      >
                        <RotateCw size={15} />
                      </IconButton>
                      <IconButton
                        label={`${t('manage')} ${service.name}`}
                        onClick={() => setSelected(id)}
                      >
                        <Settings2 size={16} />
                      </IconButton>
                      <Button
                        variant={status.status === 'running' ? 'default' : 'primary'}
                        disabled={busy}
                        onClick={() => action(id, status.status === 'running' ? 'stop' : 'start')}
                      >
                        {status.status === 'running' ? <Square size={12} /> : <Play size={12} />}
                        {t(status.status === 'running' ? 'stop' : 'start')}
                      </Button>
                    </>
                  )}
                </div>
              </div>
            );
          })}
        </div>
        {statuses.length === 0 && (
          <Empty title={t('noFilteredServices')} icon={<Search size={24} />} />
        )}
        <div className="table-footer">
          <span>
            <ShieldCheck size={13} />
            {t('integrity')}
          </span>
          <span>
            <ArrowDownToLine size={13} />
            {t('downloadOnly')}
          </span>
        </div>
      </div>
    </section>
  );
}
