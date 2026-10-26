import { lazy, Suspense, useEffect, useState } from 'react';
import {
  ArrowDownToLine,
  Check,
  Copy,
  ExternalLink,
  FolderOpen,
  KeyRound,
  LockKeyhole,
  Play,
  RotateCw,
  Save,
  ShieldCheck,
  Square,
  Trash2,
  X,
} from 'lucide-react';
import {
  Button,
  Dialog,
  DialogHeader,
  Empty,
  formatBytes,
  IconButton,
  ServiceIcon,
  Status,
} from './components';
import { Checkbox, NumberInput, Select } from './controls';
import { call, packages } from './api';
import { descriptionKeys, useApp } from './state';
import { services } from './serviceRegistry';
import { Logs } from './Logs';
import type { Credentials, ServiceId } from './types';
import type { Key } from './i18n';

const ConfigEditor = lazy(() => import('./ConfigEditor'));
export function ServiceDrawer({ id, onClose }: { id: ServiceId; onClose: () => void }) {
  const { t, snapshot, busy, run, progress, locale, toast } = useApp();
  const status = snapshot.services.find((s) => s.id === id)!;
  const config = snapshot.settings.services[id];
  const service = services[id];
  const available = packages.filter((pkg) => pkg.service === id);
  const [tab, setTab] = useState<'overview' | 'configuration' | 'logs'>('overview');
  const [installVersion, setInstallVersion] = useState(available[0]?.version ?? '');
  const [activeVersion, setActiveVersion] = useState(config.activeVersion ?? '');
  const [ports, setPorts] = useState(config.ports.map(String));
  const [autoStart, setAutoStart] = useState(config.autoStart);
  const [content, setContent] = useState('');
  const [original, setOriginal] = useState('');
  const [editorLoading, setEditorLoading] = useState(false);
  const [editorError, setEditorError] = useState('');
  const [removeVersion, setRemoveVersion] = useState<string | null>(null);
  const [discard, setDiscard] = useState(false);
  const [pendingTab, setPendingTab] = useState<typeof tab | null>(null);
  const [credentials, setCredentials] = useState<Credentials | null>(null);
  const dirty = content !== original;
  useEffect(() => {
    setActiveVersion(config.activeVersion ?? '');
    setPorts(config.ports.map(String));
    setAutoStart(config.autoStart);
    setCredentials(null);
  }, [config.activeVersion, config.ports.join(','), config.autoStart]);
  useEffect(() => {
    if (tab !== 'configuration' || !config.activeVersion) return;
    let active = true;
    setEditorLoading(true);
    setEditorError('');
    void call<string>('read_config', { id })
      .then((value) => {
        if (active) {
          setContent(value);
          setOriginal(value);
        }
      })
      .catch((error: unknown) => {
        if (active) setEditorError(String(error));
      })
      .finally(() => {
        if (active) setEditorLoading(false);
      });
    return () => {
      active = false;
    };
  }, [tab, id, config.activeVersion]);
  const selectedPackage = available.find((p) => p.version === installVersion);
  const alreadyInstalled = config.installations.some((p) => p.version === installVersion);
  const currentProgress = progress?.service === id ? progress : null;
  const close = () => {
    setPendingTab(null);
    if (dirty) setDiscard(true);
    else onClose();
  };
  const changeTab = (next: typeof tab) => {
    if (dirty) {
      setPendingTab(next);
      setDiscard(true);
      return;
    }
    setTab(next);
  };
  const saveSettings = async (event: React.FormEvent) => {
    event.preventDefault();
    await run(
      () =>
        call('save_service', {
          id,
          version: activeVersion || null,
          ports: ports.map(Number),
          autoStart,
        }),
      t('saved'),
    );
  };
  const install = () =>
    void run(
      () => call('install_package', { id, version: installVersion }),
      t('installSuccess', { name: service.name }),
    );
  const action = (action: string) =>
    void run(() => call('service_action', { id, action }), t('actionSuccess'));
  const portLabels: Key[] =
    id === 'caddy'
      ? ['portHttp', 'portHttps', 'portAdmin']
      : id === 'mailpit'
        ? ['portSmtp', 'portWeb']
        : ['portPrimary'];
  return (
    <Dialog
      title={t('serviceSettings', { name: service.name })}
      onClose={close}
      className="service-drawer"
    >
      <header className="drawer-header">
        <div className="drawer-service">
          <ServiceIcon id={id} large />
          <div>
            <h2>{service.name}</h2>
            <p>{t(descriptionKeys[id])}</p>
          </div>
        </div>
        <IconButton label={t('close')} onClick={close}>
          <X size={20} />
        </IconButton>
      </header>
      <div className="drawer-status">
        <Status status={status.status} />
        {status.pid && <code>PID {status.pid}</code>}
        <div className="drawer-status-actions">
          {config.activeVersion && (
            <>
              <IconButton
                label={t('restart')}
                disabled={busy || status.status !== 'running'}
                onClick={() => action('restart')}
              >
                <RotateCw size={16} />
              </IconButton>
              <Button
                variant={status.status === 'running' ? 'default' : 'primary'}
                disabled={busy}
                onClick={() => action(status.status === 'running' ? 'stop' : 'start')}
              >
                {status.status === 'running' ? <Square size={13} /> : <Play size={13} />}
                {t(status.status === 'running' ? 'stop' : 'start')}
              </Button>
            </>
          )}
        </div>
      </div>
      <div className="drawer-tabs" role="tablist" aria-label={service.name}>
        {(['overview', 'configuration', 'logs'] as const).map((name) => (
          <button
            key={name}
            role="tab"
            aria-selected={tab === name}
            onClick={() => changeTab(name)}
          >
            {t(name)}
          </button>
        ))}
      </div>
      <div className="drawer-body" role="tabpanel">
        {status.message && (
          <div className="inline-error" role="alert">
            {status.message}
          </div>
        )}
        {tab === 'overview' && (
          <>
            <section className="drawer-section install-section">
              <div className="section-mini-title">
                <ArrowDownToLine size={17} />
                <h3>{t('installVersion')}</h3>
                <span className="tag">x64</span>
              </div>
              <p>{t('downloadOnly')}</p>
              <label className="field-label" htmlFor="install-version">
                {t('chooseVersion')}
              </label>
              <div className="install-select">
                <Select
                  id="install-version"
                  value={installVersion}
                  disabled={busy}
                  onValueChange={setInstallVersion}
                  options={available.map((pkg) => ({
                    value: pkg.version,
                    label: pkg.version,
                    description: config.installations.some((p) => p.version === pkg.version)
                      ? t('installed')
                      : undefined,
                  }))}
                />
                <Button
                  variant="primary"
                  disabled={busy || alreadyInstalled || !selectedPackage}
                  onClick={install}
                >
                  {alreadyInstalled ? <Check size={15} /> : <ArrowDownToLine size={15} />}
                  {alreadyInstalled ? t('complete') : t('downloadInstall')}
                </Button>
              </div>
              {selectedPackage && (
                <div className="package-source">
                  <span>
                    <ShieldCheck size={14} />
                    {t('checksum')}
                  </span>
                  <span>
                    {selectedPackage.source}
                    {selectedPackage.community && <b>{t('community')}</b>}
                  </span>
                  {selectedPackage.size > 0 && (
                    <span>{formatBytes(selectedPackage.size, locale)}</span>
                  )}
                </div>
              )}
              {currentProgress && (
                <div className="download-progress" role="status">
                  <div>
                    <strong>{t(currentProgress.phase)}</strong>
                    <button onClick={() => void call('cancel_install', { id })}>
                      {t('cancel')}
                    </button>
                  </div>
                  <progress
                    max={currentProgress.total ?? undefined}
                    value={
                      currentProgress.phase === 'downloading'
                        ? currentProgress.downloaded
                        : (currentProgress.total ?? undefined)
                    }
                  />
                  <small>
                    {formatBytes(currentProgress.downloaded, locale)}
                    {currentProgress.total
                      ? ` / ${formatBytes(currentProgress.total, locale)}`
                      : ''}
                  </small>
                </div>
              )}
            </section>
            <section className="drawer-section">
              <h3>{t('settings')}</h3>
              <form onSubmit={saveSettings} className="service-settings-form">
                <label className="field-label" htmlFor="active-version">
                  {t('activeVersion')}
                </label>
                <Select
                  id="active-version"
                  value={activeVersion}
                  onValueChange={setActiveVersion}
                  disabled={busy || status.status === 'running' || !config.installations.length}
                  placeholder={t('noInstalledVersions')}
                  options={config.installations.map((pkg) => ({
                    value: pkg.version,
                    label: pkg.version,
                  }))}
                />
                <div className="ports-form">
                  {ports.map((port, index) => (
                    <div key={index}>
                      <label className="field-label" htmlFor={`${id}-port-${index}`}>
                        {t(portLabels[index])}
                      </label>
                      <NumberInput
                        id={`${id}-port-${index}`}
                        label={t(portLabels[index])}
                        min={1}
                        max={65535}
                        required
                        value={port}
                        onValueChange={(value) =>
                          setPorts((current) =>
                            current.map((old, i) => (i === index ? value : old)),
                          )
                        }
                        disabled={busy || status.status === 'running'}
                      />
                    </div>
                  ))}
                </div>
                <p className="field-help">{t('portHelp')}</p>
                <Checkbox
                  checked={autoStart}
                  onChange={(e) => setAutoStart(e.target.checked)}
                  disabled={busy || status.status === 'running'}
                >
                  {t('autoStart')}
                </Checkbox>
                <Button type="submit" disabled={busy || status.status === 'running'}>
                  <Save size={14} />
                  {t('save')}
                </Button>
              </form>
            </section>
            {config.activeVersion && (
              <section className="drawer-section">
                <h3>{t('installedPackages')}</h3>
                <p>{t('keepData')}</p>
                <div className="installed-versions">
                  {config.installations.map((pkg) => (
                    <div key={pkg.version}>
                      <span>
                        <Check size={14} />
                        <code>{pkg.version}</code>
                        {pkg.version === config.activeVersion && (
                          <small>{t('activeVersion')}</small>
                        )}
                      </span>
                      <IconButton
                        label={`${t('remove')} ${pkg.version}`}
                        disabled={busy || status.status === 'running'}
                        onClick={() => setRemoveVersion(pkg.version)}
                      >
                        <Trash2 size={15} />
                      </IconButton>
                    </div>
                  ))}
                </div>
                <div className="drawer-quick-actions">
                  <Button
                    onClick={() =>
                      void run(() => call('open_location', { id, location: 'package' }))
                    }
                  >
                    <FolderOpen size={15} />
                    {t('openFolder')}
                  </Button>
                  {['apache', 'nginx', 'mailpit'].includes(id) && (
                    <Button
                      disabled={status.status !== 'running'}
                      onClick={() => void run(() => call('open_service', { id }))}
                    >
                      <ExternalLink size={14} />
                      {t('open')}
                    </Button>
                  )}
                </div>
              </section>
            )}
            {config.activeVersion && ['mysql', 'postgresql'].includes(id) && (
              <section className="drawer-section">
                <div className="section-mini-title">
                  <KeyRound size={17} />
                  <h3>{t('credentials')}</h3>
                </div>
                <p>{t('credentialsHelp')}</p>
                {!credentials ? (
                  <Button
                    disabled={busy}
                    onClick={() =>
                      void run(async () =>
                        setCredentials(await call<Credentials>('reveal_credentials', { id })),
                      )
                    }
                  >
                    <LockKeyhole size={14} />
                    {t('reveal')}
                  </Button>
                ) : (
                  <div className="credentials">
                    <label>
                      {t('username')}
                      <code>{credentials.username}</code>
                    </label>
                    <label>
                      {t('password')}
                      <code>{credentials.password}</code>
                    </label>
                    <Button
                      onClick={() =>
                        void run(async () => {
                          await navigator.clipboard.writeText(credentials.password);
                          toast(t('copied'));
                        })
                      }
                    >
                      <Copy size={14} />
                      {t('copy')}
                    </Button>
                  </div>
                )}
              </section>
            )}
          </>
        )}
        {tab === 'configuration' &&
          (!config.activeVersion ? (
            <Empty title={t('configNotInstalled')} />
          ) : (
            <section className="editor-section">
              {editorError && (
                <div className="inline-error" role="alert">
                  {editorError}
                </div>
              )}
              <p className="field-help">{t(id === 'caddy' ? 'gatewayConfigHelp' : 'configHelp')}</p>
              <div className="editor-label">
                <code>
                  {id === 'php'
                    ? 'php.ini'
                    : id === 'apache'
                      ? 'httpd.conf'
                      : id === 'mysql'
                        ? 'my.ini'
                        : id === 'postgresql'
                          ? 'postgresql.conf'
                          : id === 'nginx'
                            ? 'nginx.conf'
                            : id === 'redis'
                              ? 'redis.conf'
                              : id === 'mailpit'
                                ? 'mailpit.json'
                                : 'Caddyfile'}
                </code>
                <span>{id === 'caddy' ? t('readOnly') : dirty ? '●' : ''}</span>
              </div>
              <Suspense fallback={<div className="editor-loading">{t('busy')}</div>}>
                <ConfigEditor
                  value={content}
                  onChange={setContent}
                  readOnly={busy || id === 'caddy'}
                  theme={document.documentElement.dataset.theme ?? 'light'}
                />
              </Suspense>
              <div className="editor-actions">
                <span>{status.status === 'running' ? t('stopBeforeEdit') : ''}</span>
                <Button
                  variant="primary"
                  disabled={
                    busy || editorLoading || !dirty || status.status === 'running' || id === 'caddy'
                  }
                  onClick={() =>
                    void run(async () => {
                      await call('save_config', { id, content });
                      setOriginal(content);
                    }, t('saved'))
                  }
                >
                  <Save size={14} />
                  {t('save')}
                </Button>
              </div>
            </section>
          ))}
        {tab === 'logs' && <Logs id={id} />}
      </div>
      {removeVersion && (
        <Dialog title={t('deletePackage')} onClose={() => setRemoveVersion(null)}>
          <DialogHeader
            title={t('confirmRemove', { name: service.name, version: removeVersion })}
            onClose={() => setRemoveVersion(null)}
          />
          <div className="dialog-content">
            <p>{t('keepData')}</p>
          </div>
          <div className="dialog-actions">
            <Button onClick={() => setRemoveVersion(null)}>{t('cancel')}</Button>
            <Button
              variant="danger"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await call('remove_package', { id, version: removeVersion });
                  setRemoveVersion(null);
                }, t('saved'))
              }
            >
              {t('confirm')}
            </Button>
          </div>
        </Dialog>
      )}
      {discard && (
        <Dialog title={t('unsaved')} onClose={() => setDiscard(false)}>
          <DialogHeader title={t('unsaved')} onClose={() => setDiscard(false)} />
          <div className="dialog-actions">
            <Button onClick={() => setDiscard(false)}>{t('cancel')}</Button>
            <Button
              variant="danger"
              onClick={() => {
                setContent(original);
                setDiscard(false);
                if (pendingTab) {
                  setTab(pendingTab);
                  setPendingTab(null);
                } else onClose();
              }}
            >
              {t('discard')}
            </Button>
          </div>
        </Dialog>
      )}
    </Dialog>
  );
}
