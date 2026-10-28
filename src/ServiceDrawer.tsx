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
import { call, packages, userPathApi } from './api';
import { descriptionKeys, useApp } from './state';
import { services } from './serviceRegistry';
import { Logs } from './Logs';
import { PhpExtensions } from './PhpExtensions';
import { serviceIds, type Credentials, type ServiceId, type PathStatus } from './types';
import type { Key } from './i18n';

const ConfigEditor = lazy(() => import('./ConfigEditor'));
export function ServiceDrawer({ id, onClose }: { id: ServiceId; onClose: () => void }) {
  const { t, snapshot, busy, run, progress, locale, toast, setSelected } = useApp();
  const status = snapshot.services.find((s) => s.id === id)!;
  const config = snapshot.settings.services[id];
  const service = services[id];
  const dependentPaths = serviceIds.filter(
    (dependent) =>
      (services[dependent].dependencies as string[]).includes(id) &&
      snapshot.settings.services[dependent].addToPath,
  );
  const dependenciesReady = service.dependencies.every(
    (dependency) => snapshot.settings.services[dependency as ServiceId].activeVersion,
  );
  const dependencyVersions = service.dependencies
    .map((dependency) => snapshot.settings.services[dependency as ServiceId].activeVersion)
    .join(',');
  const available = packages.filter((pkg) => pkg.service === id);
  const [tab, setTab] = useState<'overview' | 'extensions' | 'configuration' | 'logs'>('overview');
  const [installVersion, setInstallVersion] = useState(available[0]?.version ?? '');
  const [activeVersion, setActiveVersion] = useState(config.activeVersion ?? '');
  const [ports, setPorts] = useState(config.ports.map(String));
  const [autoStart, setAutoStart] = useState(config.autoStart);
  const [registerAfterInstall, setRegisterAfterInstall] = useState(false);
  const [pathStatus, setPathStatus] = useState<PathStatus | null>(null);
  const [pathError, setPathError] = useState('');
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
    let active = true;
    setPathStatus(null);
    setPathError('');
    if (!config.activeVersion || !dependenciesReady) return;
    void userPathApi
      .status(id)
      .then((value) => {
        if (active) setPathStatus(value);
      })
      .catch((error: unknown) => {
        if (active) setPathError(String(error));
      });
    return () => {
      active = false;
    };
  }, [id, config.activeVersion, config.addToPath, dependenciesReady, dependencyVersions]);
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
      async () => {
        await userPathApi.install(id, installVersion, registerAfterInstall);
        setRegisterAfterInstall(false);
      },
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
          {config.activeVersion && service.daemon && (
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
        {(
          [
            'overview',
            ...(id === 'php' ? ['extensions' as const] : []),
            ...(service.configName ? ['configuration' as const] : []),
            ...(service.daemon ? ['logs' as const] : []),
          ] as const
        ).map((name) => (
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
        <div data-notification-host />
        {tab === 'extensions' && id === 'php' && <PhpExtensions version={config.activeVersion} />}
        {!service.daemon && (
          <section className="drawer-section">
            <p>{t('terminalToolHelp')}</p>
            {id === 'nodejs' && <p>{t('nodeNpmHelp')}</p>}
            {id === 'composer' && (
              <>
                <p>
                  {t('composerPhpHelp', {
                    version: snapshot.settings.services.php.activeVersion ?? '—',
                  })}
                </p>
                {!dependenciesReady && <p className="field-help">{t('toolDependencyRequired')}</p>}
                <Button onClick={() => setSelected('php')}>{t('managePhp')}</Button>
              </>
            )}
          </section>
        )}
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
              {selectedPackage &&
                'bundledTools' in selectedPackage &&
                selectedPackage.bundledTools && (
                  <p className="field-help">
                    {t('bundledTool', {
                      name: 'npm',
                      version: selectedPackage.bundledTools.npm ?? '—',
                    })}
                  </p>
                )}
              <div className="path-install-option">
                <Checkbox
                  checked={config.addToPath || registerAfterInstall}
                  disabled={busy || config.addToPath || alreadyInstalled || !dependenciesReady}
                  onChange={(event) => setRegisterAfterInstall(event.target.checked)}
                >
                  {t('pathInstallOption')}
                </Checkbox>
                <p className="field-help">
                  {t('pathInstallHelp', {
                    name: service.name,
                    version: config.activeVersion ?? installVersion,
                  })}
                </p>
              </div>
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
                {service.daemon && (
                  <>
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
                  </>
                )}
                <Button type="submit" disabled={busy || status.status === 'running'}>
                  <Save size={14} />
                  {t('save')}
                </Button>
              </form>
            </section>
            {config.activeVersion && (
              <section className="drawer-section path-section">
                <h3>{t('userPathTitle')}</h3>
                <p>{t('userPathHelp')}</p>
                <Checkbox
                  checked={config.addToPath ?? false}
                  disabled={busy || (!config.addToPath && !dependenciesReady)}
                  onChange={(event) =>
                    void run(async () => {
                      await userPathApi.set(id, event.target.checked);
                      setPathStatus(await userPathApi.status(id));
                    }, t('pathUpdated'))
                  }
                >
                  {t('pathFollowActive', { name: service.name })}
                </Checkbox>
                <p className="field-help">
                  {t('pathVersion', { name: service.name, version: config.activeVersion })}
                </p>
                {pathError && (
                  <p className="inline-error" role="alert">
                    {pathError}
                  </p>
                )}
                {(
                  pathStatus?.directories ?? (pathStatus?.directory ? [pathStatus.directory] : [])
                ).map((directory) => (
                  <code key={directory} className="path-directory">
                    {directory}
                  </code>
                ))}
                {config.addToPath && pathStatus && !pathStatus.present && (
                  <div className="inline-error">
                    <p>{t('pathMissing')}</p>
                    <Button
                      disabled={busy}
                      onClick={() =>
                        void run(async () => {
                          await userPathApi.set(id, true);
                          setPathStatus(await userPathApi.status(id));
                        }, t('pathUpdated'))
                      }
                    >
                      {t('pathRepair')}
                    </Button>
                  </div>
                )}
                {pathStatus?.shadowed && (
                  <div className="path-conflict">
                    <p>{t('pathConflict', { command: pathStatus.command })}</p>
                    <code className="path-directory">{pathStatus.firstMatch}</code>
                  </div>
                )}
                {pathStatus?.commands
                  ?.filter((command) => command.shadowed && command.command !== pathStatus.command)
                  .map((command) => (
                    <div className="path-conflict" key={command.command}>
                      <p>{t('pathConflict', { command: command.command })}</p>
                      <code className="path-directory">{command.firstMatch}</code>
                    </div>
                  ))}
                {pathStatus?.external && <p className="field-help">{t('pathExternal')}</p>}
                {config.addToPath && <p className="field-help">{t('pathNewTerminal')}</p>}
                {config.addToPath && pathStatus?.phpConfig && (
                  <p className="field-help">
                    {t('pathPhpConfig')}
                    <code className="path-directory">{pathStatus.phpConfig}</code>
                  </p>
                )}
              </section>
            )}
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
              <p className="field-help">
                {t(
                  id === 'caddy'
                    ? 'gatewayConfigHelp'
                    : id === 'nginx'
                      ? 'nginxConfigHelp'
                      : 'configHelp',
                )}
              </p>
              <div className="editor-label">
                <code>{service.configName}</code>
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
            {config.installations.length === 1 && dependentPaths.length > 0 && (
              <p role="alert">
                {t('dependencyPathInUse', {
                  names: dependentPaths.map((dependent) => services[dependent].name).join(', '),
                })}
              </p>
            )}
            {config.addToPath && removeVersion === config.activeVersion && (
              <p>
                {config.installations.some((pkg) => pkg.version !== removeVersion)
                  ? t('pathRemoveFallback', {
                      name: service.name,
                      version: config.installations.find((pkg) => pkg.version !== removeVersion)!
                        .version,
                    })
                  : t('pathRemoveLast')}
              </p>
            )}
          </div>
          <div className="dialog-actions">
            <Button onClick={() => setRemoveVersion(null)}>{t('cancel')}</Button>
            <Button
              variant="danger"
              disabled={busy || (config.installations.length === 1 && dependentPaths.length > 0)}
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
