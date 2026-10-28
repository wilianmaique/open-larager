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
import { Checkbox, NumberInput, RadioCard, Switch } from './controls';
import { call } from './api';
import { useApp } from './state';
import type { Site } from './types';
import { SiteConfigDialog } from './SiteConfigDialog';

export function Domains() {
  const { t, snapshot, run, busy, setSelected } = useApp();
  const [editing, setEditing] = useState<Site | 'new' | null>(null);
  const [certificate, setCertificate] = useState<string | null>(null);
  const [configuring, setConfiguring] = useState<Site | null>(null);
  const gateway = snapshot.services.find((s) => s.id === 'caddy')!;
  const nginx = snapshot.services.find((s) => s.id === 'nginx')!;
  const php = snapshot.services.find((s) => s.id === 'php')!;
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
                  {site.server === 'nginx' ? 'Nginx · ' : 'Caddy · '}
                  {t(
                    site.mode === 'php'
                      ? 'phpSite'
                      : site.mode === 'static'
                        ? 'staticSite'
                        : 'proxySite',
                  )}
                </span>
                <Switch
                  title={t('enabled')}
                  aria-label={`${t('enabled')} ${site.hostname}`}
                  checked={site.enabled}
                  disabled={busy || (site.server === 'nginx' && nginx.status === 'running')}
                  onChange={(e) =>
                    void run(
                      () => call('save_site', { site: { ...site, enabled: e.target.checked } }),
                      t('saved'),
                    )
                  }
                />
                <IconButton
                  label={`${t('edit')} ${site.hostname}`}
                  onClick={() => setEditing(site)}
                >
                  <Pencil size={15} />
                </IconButton>
                {site.server === 'nginx' && (
                  <IconButton
                    label={`${t('siteConfiguration')} ${site.hostname}`}
                    onClick={() => setConfiguring(site)}
                  >
                    <FileCode2 size={15} />
                  </IconButton>
                )}
                <IconButton
                  label={`${t('open')} ${site.hostname}`}
                  disabled={
                    !site.enabled ||
                    gateway.status !== 'running' ||
                    !snapshot.hostsSynced ||
                    (site.server === 'nginx' && nginx.status !== 'running') ||
                    (site.mode === 'php' && php.status !== 'running')
                  }
                  onClick={() => void run(() => call('open_site', { id: site.id }))}
                >
                  <ExternalLink size={15} />
                </IconButton>
                <IconButton
                  label={`${t('remove')} ${site.hostname}`}
                  disabled={busy || (site.server === 'nginx' && nginx.status === 'running')}
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
      {snapshot.settings.sites.some((site) => site.server === 'nginx') && (
        <div className="info-strip site-service-hint">
          <Info size={16} />
          <span>{t(nginx.status === 'running' ? 'siteStopNginx' : 'siteNginxNeeded')}</span>
          <Button onClick={() => setSelected('nginx')}>{t('manageNginx')}</Button>
        </div>
      )}
      {snapshot.settings.sites.length > 0 && gateway.status !== 'running' && (
        <div className="info-strip">
          <Info size={16} />
          {t('gatewayNeeded')}
        </div>
      )}
      {editing && (
        <SiteForm site={editing === 'new' ? undefined : editing} onClose={() => setEditing(null)} />
      )}
      {configuring && <SiteConfigDialog site={configuring} onClose={() => setConfiguring(null)} />}
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
  const { t, busy, run, snapshot } = useApp();
  const [form, setForm] = useState<Site>(
    site
      ? { ...site, server: site.server ?? 'caddy' }
      : {
          id: '',
          hostname: '',
          root: '',
          mode: 'php',
          upstreamPort: 3000,
          enabled: true,
          server: 'nginx',
          nginxDirectives: null,
        },
  );
  const nginxRunning = snapshot.services.some(
    (service) => service.id === 'nginx' && service.status === 'running',
  );
  const blocked = nginxRunning && (form.server === 'nginx' || site?.server === 'nginx');
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
            <legend className="field-label">{t('siteServer')}</legend>
            <div className="mode-options site-server-options">
              <RadioCard
                name="site-server"
                value="nginx"
                checked={form.server === 'nginx'}
                disabled={busy}
                onChange={() => update('server', 'nginx')}
              >
                <span aria-hidden="true">
                  <ServiceIcon id="nginx" />
                </span>
                <span>{t('siteServerNginx')}</span>
              </RadioCard>
              <RadioCard
                name="site-server"
                value="caddy"
                checked={form.server === 'caddy'}
                disabled={busy}
                onChange={() => update('server', 'caddy')}
              >
                <span aria-hidden="true">
                  <ServiceIcon id="caddy" />
                </span>
                <span>{t('siteServerCaddy')}</span>
              </RadioCard>
            </div>
            <p className="field-help">
              {t(form.server === 'nginx' ? 'siteServerNginxHelp' : 'siteServerCaddyHelp')}
            </p>
          </fieldset>
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
                <RadioCard
                  key={value}
                  name="site-mode"
                  value={value}
                  checked={form.mode === value}
                  disabled={busy || site?.nginxDirectives != null}
                  onChange={() => update('mode', value)}
                >
                  <Icon size={21} />
                  <span>{t(label)}</span>
                </RadioCard>
              ))}
            </div>
          </fieldset>
          {site?.nginxDirectives != null && (
            <p className="field-help">{t('siteResetBeforeType')}</p>
          )}
          {form.mode === 'proxy' ? (
            <>
              <label className="field-label" htmlFor="site-port">
                {t('upstreamPort')}
              </label>
              <NumberInput
                id="site-port"
                label={t('upstreamPort')}
                min={1}
                max={65535}
                required
                value={form.upstreamPort ?? ''}
                disabled={busy}
                onValueChange={(value) =>
                  update('upstreamPort', value === '' ? null : Number(value))
                }
              />
            </>
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
          <Checkbox
            checked={form.enabled}
            disabled={busy}
            onChange={(e) => update('enabled', e.target.checked)}
          >
            {t('enabled')}
          </Checkbox>
          {blocked && (
            <p className="field-help" role="status">
              {t('siteStopNginx')}
            </p>
          )}
        </div>
        <div className="dialog-actions">
          <Button type="button" onClick={onClose}>
            {t('cancel')}
          </Button>
          <Button type="submit" variant="primary" disabled={busy || blocked}>
            <Check size={15} />
            {t('saveDomain')}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
