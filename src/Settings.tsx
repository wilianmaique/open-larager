import {
  ArrowRight,
  FolderPen,
  FolderOpen,
  Globe2,
  HardDrive,
  Laptop,
  Moon,
  Palette,
  PanelBottomClose,
  RotateCcw,
  ShieldCheck,
  Sun,
} from 'lucide-react';
import { useRef, useState } from 'react';
import { Button, Dialog, DialogHeader } from './components';
import { Checkbox, Select } from './controls';
import { call, configurationApi, desktopApi, environmentApi, native } from './api';
import type {
  EnvironmentFolder,
  EnvironmentMove,
  EnvironmentMoveProgress as MoveProgress,
  ConfigurationReset,
} from './types';
import { EnvironmentMoveProgress } from './EnvironmentMoveProgress';
import { FileFailureDetails } from './FileFailureDetails';
import { useApp } from './state';
import type { Key } from './i18n';
import catalog from '../src-tauri/catalog/packages.json';

export function Settings() {
  const { t, snapshot, preferences, busy, run, locale } = useApp();
  const [destination, setDestination] = useState<EnvironmentFolder | null>(null);
  const [moveStatus, setMoveStatus] = useState<EnvironmentMove | null>(null);
  const [moveProgress, setMoveProgress] = useState<MoveProgress | null>(null);
  const [resetOpen, setResetOpen] = useState(false);
  const [resetConfirmed, setResetConfirmed] = useState(false);
  const [resetResult, setResetResult] = useState<ConfigurationReset | null>(null);
  const resetting = useRef(false);
  const running = snapshot.services.some((service) => service.status === 'running');
  const moveState = moveStatus ?? snapshot.environmentMove;
  const cleanup = moveState?.cleanupSource;
  const chooseEnvironment = () =>
    run(async () => {
      const folder = await environmentApi.choose();
      if (folder) setDestination(folder);
    });
  const moveEnvironment = () =>
    run(async () => {
      if (!destination) return;
      setMoveProgress(null);
      try {
        const status = await environmentApi.move(destination.token, setMoveProgress);
        setMoveStatus(status);
        setDestination(null);
      } finally {
        setMoveProgress(null);
      }
    }, t('environmentMoved'));
  const closeReset = () => {
    if (resetting.current) return;
    setResetOpen(false);
    setResetConfirmed(false);
  };
  const resetConfiguration = async () => {
    if (resetting.current || !resetConfirmed || busy || cleanup) return;
    resetting.current = true;
    try {
      await run(async () => {
        const result = await configurationApi.reset();
        setResetResult(result);
        setResetOpen(false);
        setResetConfirmed(false);
      });
    } finally {
      resetting.current = false;
    }
  };
  return (
    <>
      <div className="page-heading">
        <div>
          <span className="section-eyebrow">{t('system')}</span>
          <h1>{t('preferences')}</h1>
          <p>{t('preferencesDescription')}</p>
        </div>
      </div>
      <section className="settings-panel">
        <div className="settings-section">
          <div className="setting-icon">
            <Palette size={21} />
          </div>
          <div className="setting-content">
            <h2>{t('appearance')}</h2>
            <p>{t('appearanceDescription')}</p>
            <div className="theme-options">
              {(
                [
                  { value: 'light', label: 'light', Icon: Sun },
                  { value: 'dark', label: 'dark', Icon: Moon },
                  { value: 'system', label: 'systemTheme', Icon: Laptop },
                ] as const
              ).map(({ value, label, Icon }) => (
                <button
                  disabled={busy}
                  key={value}
                  className={snapshot.settings.theme === value ? 'selected' : ''}
                  onClick={() => void preferences(snapshot.settings.language, value)}
                >
                  <span className={`theme-preview theme-preview-${value}`}>
                    <i />
                    <i />
                    <i />
                  </span>
                  <span>
                    <Icon size={15} />
                    {t(label)}
                    <i className="radio-dot" />
                  </span>
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="settings-section">
          <div className="setting-icon">
            <Globe2 size={21} />
          </div>
          <div className="setting-content">
            <h2>{t('language')}</h2>
            <p>{t('languageDescription')}</p>
            <Select
              className="language-select"
              aria-label={t('language')}
              value={snapshot.settings.language}
              disabled={busy}
              onValueChange={(language) => void preferences(language, snapshot.settings.theme)}
              options={[
                { value: 'auto', label: `${t('autoLanguage')} (${snapshot.systemLocale})` },
                { value: 'en', label: t('languageEnglish') },
                { value: 'pt-BR', label: t('languagePortuguese') },
                { value: 'es', label: t('languageSpanish') },
                { value: 'zh-CN', label: t('languageChinese') },
                { value: 'hi', label: t('languageHindi') },
              ]}
            />
          </div>
        </div>
      </section>
      <section className="settings-panel">
        <div className="settings-section">
          <div className="setting-icon">
            <PanelBottomClose size={21} />
          </div>
          <div className="setting-content">
            <h2>{t('closeBehavior')}</h2>
            <p id="close-behavior-help">{t('closeBehaviorDescription')}</p>
            <Select
              className="close-behavior-select"
              aria-label={t('closeBehavior')}
              aria-describedby="close-behavior-help"
              value={snapshot.settings.closeBehavior}
              disabled={busy || !native}
              onValueChange={(behavior) =>
                void run(() => desktopApi.saveCloseBehavior(behavior), t('saved'))
              }
              options={[
                { value: 'ask', label: t('askBeforeClosing') },
                { value: 'tray', label: t('minimizeToTray') },
                { value: 'exit', label: t('exitApp') },
              ]}
            />
          </div>
        </div>
      </section>
      <section className="settings-panel storage-panel">
        <div className="settings-section">
          <div className="setting-icon">
            <HardDrive size={21} />
          </div>
          <div className="setting-content">
            <div className="storage-heading">
              <div>
                <h2>{t('storage')}</h2>
                <p>{t('storageDescription')}</p>
              </div>
              <div className="storage-actions">
                <Button
                  disabled={busy || !snapshot.root}
                  onClick={() => void run(() => call('open_location', { location: 'workspace' }))}
                >
                  <FolderOpen size={15} />
                  {t('openWorkspace')}
                </Button>
                <Button
                  disabled={
                    busy ||
                    running ||
                    !!snapshot.busyService ||
                    !!cleanup ||
                    !native ||
                    !snapshot.root
                  }
                  onClick={() => void chooseEnvironment()}
                >
                  <FolderPen size={15} />
                  {t('changeEnvironment')}
                </Button>
              </div>
            </div>
            {snapshot.root && <code className="workspace-path">{snapshot.root}</code>}
            <p className="field-help">
              {t(running ? 'environmentStopFirst' : 'environmentMoveHelp')}
            </p>
            {cleanup && (
              <div className="environment-cleanup" role="alert">
                <p>{t('environmentCleanupPending')}</p>
                <code>{cleanup}</code>
                {moveState?.cleanupIssue ? (
                  <FileFailureDetails issue={moveState.cleanupIssue} t={t} />
                ) : (
                  moveState?.cleanupError && <p>{moveState.cleanupError}</p>
                )}
                <Button
                  disabled={busy || running}
                  loading={busy}
                  onClick={() =>
                    void run(async () => setMoveStatus(await environmentApi.cleanup()))
                  }
                >
                  {t('environmentRetryCleanup')}
                </Button>
              </div>
            )}
            <div className="directory-list">
              {[
                { dir: 'packages/', label: 'pathPackages' },
                { dir: 'config/', label: 'pathConfig' },
                { dir: 'data/', label: 'pathData' },
                { dir: 'logs/', label: 'pathLogs' },
                { dir: 'projects/', label: 'pathProjects' },
                { dir: 'secrets/', label: 'pathSecrets' },
              ].map(({ dir, label }) => (
                <div key={dir}>
                  <FolderOpen size={14} />
                  <code>{dir}</code>
                  <span>{t(label as Key)}</span>
                </div>
              ))}
            </div>
            <div className="portable-note">
              <ShieldCheck size={19} />
              <div>
                <h3>{t('portableTitle')}</h3>
                <p>{t('portableDescription')}</p>
              </div>
            </div>
          </div>
        </div>
      </section>
      <section className="settings-panel configuration-reset-panel">
        <div className="settings-section">
          <div className="setting-icon">
            <RotateCcw size={21} />
          </div>
          <div className="setting-content">
            <h2>{t('resetConfiguration')}</h2>
            <p>{t('resetConfigurationDescription')}</p>
            <Button
              variant="danger"
              disabled={busy || !native || !snapshot.root || !!snapshot.busyService || !!cleanup}
              onClick={() => {
                setResetConfirmed(false);
                setResetOpen(true);
              }}
            >
              <RotateCcw size={15} />
              {t('resetConfiguration')}
            </Button>
            {cleanup && <p className="field-help">{t('resetBlocked')}</p>}
            {resetResult && (
              <div className="configuration-reset-result" role="status">
                <p>{t('resetSuccess')}</p>
                <strong>{t('resetBackupLabel')}</strong>
                <code>{resetResult.backupPath}</code>
                <Button
                  disabled={busy}
                  onClick={() => void run(() => call('open_location', { location: 'backups' }))}
                >
                  <FolderOpen size={15} />
                  {t('resetOpenBackups')}
                </Button>
              </div>
            )}
          </div>
        </div>
      </section>
      {resetOpen && (
        <Dialog
          title={t('resetConfirmTitle')}
          className="configuration-reset-dialog"
          onClose={closeReset}
        >
          <DialogHeader title={t('resetConfirmTitle')} onClose={closeReset} />
          <div className="dialog-content">
            <code className="workspace-path">{snapshot.root}</code>
            <h3>{t('resetChangesTitle')}</h3>
            <ul>
              <li>{t('resetChangesPreferences')}</li>
              <li>{t('resetChangesServices')}</li>
              <li>{t('resetChangesFiles')}</li>
              <li>{t('resetChangesSites')}</li>
            </ul>
            <h3>{t('resetPreservedTitle')}</h3>
            <ul>
              <li>{t('resetPreservePackages')}</li>
              <li>{t('resetPreserveData')}</li>
              <li>{t('resetPreserveExternal')}</li>
            </ul>
            <p className="field-help">{t('resetStopHelp')}</p>
            <p className="field-help">{t('resetBackupHelp')}</p>
            <Checkbox
              checked={resetConfirmed}
              disabled={busy}
              onChange={(event) => setResetConfirmed(event.target.checked)}
            >
              {t('resetConfirmation')}
            </Checkbox>
          </div>
          <div data-notification-host />
          <div className="dialog-actions">
            <Button disabled={busy} onClick={closeReset}>
              {t('cancel')}
            </Button>
            <Button
              variant="danger"
              disabled={busy || !resetConfirmed || !!cleanup}
              loading={busy}
              onClick={() => void resetConfiguration()}
            >
              {t(busy ? 'resetBusy' : 'resetConfiguration')}
            </Button>
          </div>
        </Dialog>
      )}
      {destination && (
        <Dialog
          title={t('environmentMoveTitle')}
          className="environment-move-dialog"
          onClose={() => {
            if (!busy) setDestination(null);
          }}
        >
          <DialogHeader
            title={t('environmentMoveTitle')}
            onClose={() => {
              if (!busy) setDestination(null);
            }}
          />
          <div className="dialog-content environment-move-content">
            <p>{t('environmentMoveConfirm')}</p>
            <div className="environment-move-paths">
              <div>
                <strong>{t('environmentCurrentFolder')}</strong>
                <code>{snapshot.root}</code>
              </div>
              <ArrowRight size={18} aria-hidden="true" />
              <div>
                <strong>{t('environmentNewFolder')}</strong>
                <code>{destination.path}</code>
              </div>
            </div>
            <p className="field-help">{t('environmentMoveCaution')}</p>
            {busy && <EnvironmentMoveProgress progress={moveProgress} locale={locale} t={t} />}
          </div>
          <div data-notification-host />
          <div className="dialog-actions">
            <Button disabled={busy} onClick={() => setDestination(null)}>
              {t('cancel')}
            </Button>
            <Button
              variant="primary"
              disabled={busy || running}
              loading={busy}
              onClick={() => void moveEnvironment()}
            >
              {t(busy ? 'environmentMoving' : 'environmentMoveAction')}
            </Button>
          </div>
        </Dialog>
      )}
      <div className="about-card">
        <img src="/brand.svg" alt="" />
        <div>
          <strong>{t('about')}</strong>
          <p>{t('aboutDescription')}</p>
          <small>
            {t('catalogDate', {
              date: new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(
                new Date(catalog.generatedAt),
              ),
            })}
          </small>
        </div>
        <span>v0.1.0</span>
      </div>
    </>
  );
}
