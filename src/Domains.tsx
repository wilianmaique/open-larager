import { useState } from 'react';
import {
  ArrowRight,
  Check,
  ChevronRight,
  Code2,
  ExternalLink,
  FileCode2,
  FolderOpen,
  Globe2,
  Info,
  LockKeyhole,
  Pencil,
  Play,
  Plus,
  ShieldCheck,
  Trash2,
  Waypoints,
} from 'lucide-react';
import { Button, Dialog, DialogHeader, Empty, IconButton, ServiceIcon, Status } from './components';
import { call } from './api';
import { useApp } from './state';
import type { Site } from './types';

export function Domains() {
  const { t, snapshot, run, busy, setSelected } = useApp();
  const [editing, setEditing] = useState<Site | 'new' | null>(null);
  const [certificate, setCertificate] = useState<string | null>(null);
  const gateway = snapshot.services.find((s) => s.id === 'caddy')!;
  const certificateAction = (action: string) =>
    void run(async () => {
      const result = await call<string>('certificate_action', { action });
      if (action === 'inspect') setCertificate(result);
    }, t('certificateTrusted'));
  return (
    <>
      <div className="page-heading">
        <div>
          <span className="section-eyebrow">{t('environment')}</span>
          <h1>{t('nav.domains')}</h1>
          <p>{t('domainsDescription')}</p>
        </div>
        <Button variant="primary" onClick={() => setEditing('new')}>
          <Plus size={16} />
          {t('addDomain')}
        </Button>
      </div>
      <div className="domains-top-grid">
        <section className="gateway-card">
          <div className="gateway-top">
            <ServiceIcon id="caddy" />
            <div>
              <h2>{t('httpsGateway')}</h2>
              <span>{t('sslFlow')}</span>
            </div>
            <Status status={gateway.status} />
          </div>
          <p>{t('httpsDescription')}</p>
          <div className="gateway-actions">
            {!gateway.version ? (
              <Button variant="primary" onClick={() => setSelected('caddy')}>
                <ArrowRight size={15} />
                {t('gatewayInstall')}
              </Button>
            ) : gateway.status !== 'running' ? (
              <Button
                variant="primary"
                disabled={busy}
                onClick={() =>
                  void run(
                    () => call('service_action', { id: 'caddy', action: 'start' }),
                    t('actionSuccess'),
                  )
                }
              >
                <Play size={14} />
                {t('gatewayStart')}
              </Button>
            ) : (
              <Button
                disabled={busy || snapshot.certificate.trusted}
                onClick={() => certificateAction('trust')}
              >
                <ShieldCheck size={15} />
                {t(snapshot.certificate.trusted ? 'caTrusted' : 'trustCertificate')}
              </Button>
            )}
            <Button
              variant="ghost"
              disabled={busy || !snapshot.certificate.available}
              onClick={() => certificateAction('inspect')}
            >
              {t('certificateDetails')}
              <ChevronRight size={14} />
            </Button>
          </div>
          <small className="field-help">{t('certificateHelp')}</small>
          {gateway.version && (
            <button
              className="text-button untrust-button"
              disabled={busy || !snapshot.certificate.trusted}
              title={t('trustRemoveHelp')}
              onClick={() => certificateAction('untrust')}
            >
              {t('certificateRemove')}
            </button>
          )}
        </section>
        <section className="hosts-card">
          <div className="hosts-heading">
            <span className="hosts-icon">
              <Waypoints size={22} />
            </span>
            <span className={`sync-tag ${snapshot.hostsSynced ? 'synced' : ''}`}>
              {snapshot.hostsSynced && <Check size={13} />}
              {t(snapshot.hostsSynced ? 'hostsSynced' : 'hostsPending')}
            </span>
          </div>
          <h2>hosts</h2>
          <p>{t('hostsDescription')}</p>
          <div className="hosts-preview">
            <span>127.0.0.1</span>
            <code>
              {snapshot.settings.sites.find((s) => s.enabled)?.hostname ?? 'my-site.test'}
            </code>
          </div>
          <Button
            disabled={busy || snapshot.hostsSynced}
            onClick={() => void run(() => call('apply_hosts'), t('hostsApplied'))}
          >
            <ShieldCheck size={14} />
            {t('applyHosts')}
          </Button>
        </section>
      </div>
      <section className="domain-list-section">
        <div className="section-heading">
          <h2>
            {t('localDomains')}
            <span className="count-badge">{snapshot.settings.sites.length}</span>
          </h2>
          <span className="muted small">.test / HTTPS</span>
        </div>
        <div className="domain-panel">
          {snapshot.settings.sites.length === 0 ? (
            <Empty
              icon={<Globe2 size={30} />}
              title={t('noDomains')}
              description={t('noDomainsDescription')}
            >
              <Button onClick={() => setEditing('new')}>
                <Plus size={15} />
                {t('addDomain')}
              </Button>
            </Empty>
          ) : (
            snapshot.settings.sites.map((site) => (
              <div className="domain-row" key={site.id}>
                <span className={`domain-icon ${site.enabled ? '' : 'domain-disabled'}`}>
                  <Globe2 size={23} />
                  <LockKeyhole size={11} />
                </span>
                <div className="domain-name">
                  <strong>{site.hostname}</strong>
                  <span>
                    {site.mode === 'proxy' ? `127.0.0.1:${site.upstreamPort}` : site.root}
                  </span>
                </div>
                <span className="tag">
                  {t(
                    site.mode === 'php'
                      ? 'phpSite'
                      : site.mode === 'static'
                        ? 'staticSite'
                        : 'proxySite',
                  )}
                </span>
                <label className="switch" title={t('enabled')}>
                  <input
                    type="checkbox"
                    aria-label={`${t('enabled')} ${site.hostname}`}
                    checked={site.enabled}
                    disabled={busy}
                    onChange={(e) =>
                      void run(
                        () => call('save_site', { site: { ...site, enabled: e.target.checked } }),
                        t('saved'),
                      )
                    }
                  />
                  <span />
                </label>
                <IconButton
                  label={`${t('edit')} ${site.hostname}`}
                  onClick={() => setEditing(site)}
                >
                  <Pencil size={15} />
                </IconButton>
                <IconButton
                  label={`${t('open')} ${site.hostname}`}
                  disabled={!site.enabled || gateway.status !== 'running' || !snapshot.hostsSynced}
                  onClick={() => void run(() => call('open_site', { id: site.id }))}
                >
                  <ExternalLink size={15} />
                </IconButton>
                <IconButton
                  label={`${t('remove')} ${site.hostname}`}
                  disabled={busy}
                  onClick={() =>
                    void run(() => call('remove_site', { id: site.id }), t('domainRemoved'))
                  }
                >
                  <Trash2 size={15} />
                </IconButton>
              </div>
            ))
          )}
        </div>
      </section>
      {snapshot.settings.sites.length > 0 && gateway.status !== 'running' && (
        <div className="info-strip">
          <Info size={16} />
          {t('gatewayNeeded')}
        </div>
      )}
      {editing && (
        <SiteForm site={editing === 'new' ? undefined : editing} onClose={() => setEditing(null)} />
      )}
      {certificate !== null && (
        <Dialog
          title={t('certificateDetails')}
          onClose={() => setCertificate(null)}
          className="certificate-dialog"
        >
          <DialogHeader title={t('certificateDetails')} onClose={() => setCertificate(null)} />
          <pre className="certificate-output">{certificate}</pre>
        </Dialog>
      )}
    </>
  );
}

function SiteForm({ site, onClose }: { site?: Site; onClose: () => void }) {
  const { t, busy, run } = useApp();
  const [form, setForm] = useState<Site>(
    site ?? { id: '', hostname: '', root: '', mode: 'php', upstreamPort: 3000, enabled: true },
  );
  const update = <K extends keyof Site>(key: K, value: Site[K]) =>
    setForm((current) => ({ ...current, [key]: value }));
  return (
    <Dialog title={t(site ? 'editDomain' : 'addDomain')} onClose={onClose} className="site-dialog">
      <DialogHeader
        title={t(site ? 'editDomain' : 'addDomain')}
        subtitle={t('domainsTitle')}
        onClose={onClose}
      />
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void run(async () => {
            await call('save_site', { site: form });
            onClose();
          }, t('domainSaved'));
        }}
      >
        <div className="dialog-content">
          <label className="field-label" htmlFor="site-host">
            {t('hostname')}
          </label>
          <div className="input-prefix">
            <span>https://</span>
            <input
              id="site-host"
              autoFocus
              placeholder="my-site.test"
              required
              pattern="[a-zA-Z0-9][a-zA-Z0-9.\-]*\.[a-zA-Z][a-zA-Z0-9.\-]*"
              maxLength={253}
              value={form.hostname}
              onChange={(e) => update('hostname', e.target.value)}
            />
          </div>
          <p className="field-help">{t('domainHelp')}</p>
          <fieldset className="mode-fieldset">
            <legend className="field-label">{t('siteType')}</legend>
            <div className="mode-options">
              {(
                [
                  { value: 'php', label: 'phpSite', Icon: Code2 },
                  { value: 'static', label: 'staticSite', Icon: FileCode2 },
                  { value: 'proxy', label: 'proxySite', Icon: Waypoints },
                ] as const
              ).map(({ value, label, Icon }) => (
                <label key={value} className={form.mode === value ? 'selected' : ''}>
                  <input
                    type="radio"
                    name="site-mode"
                    checked={form.mode === value}
                    onChange={() => update('mode', value)}
                  />
                  <Icon size={21} />
                  <span>{t(label)}</span>
                </label>
              ))}
            </div>
          </fieldset>
          {form.mode === 'proxy' ? (
            <label className="field-label">
              {t('upstreamPort')}
              <input
                type="number"
                min="1"
                max="65535"
                required
                value={form.upstreamPort ?? ''}
                onChange={(e) => update('upstreamPort', Number(e.target.value))}
              />
            </label>
          ) : (
            <>
              <label className="field-label" htmlFor="site-root">
                {t('projectFolder')}
              </label>
              <div className="folder-input">
                <input
                  id="site-root"
                  required
                  value={form.root}
                  onChange={(e) => update('root', e.target.value)}
                  placeholder="C:\Projects\my-site\public"
                />
                <Button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      const path = await call<string | null>('choose_folder');
                      if (path) update('root', path);
                    })
                  }
                >
                  <FolderOpen size={15} />
                  {t('browse')}
                </Button>
              </div>
              <p className="field-help">{t('folderHelp')}</p>
            </>
          )}
          <label className="checkbox">
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(e) => update('enabled', e.target.checked)}
            />
            <span>{t('enabled')}</span>
          </label>
        </div>
        <div className="dialog-actions">
          <Button type="button" onClick={onClose}>
            {t('cancel')}
          </Button>
          <Button type="submit" variant="primary" disabled={busy}>
            <Check size={15} />
            {t('saveDomain')}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
