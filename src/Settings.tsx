import {
  FolderOpen,
  Globe2,
  HardDrive,
  Laptop,
  Moon,
  Palette,
  ShieldCheck,
  Sun,
} from 'lucide-react';
import { Button } from './components';
import { Select } from './controls';
import { call } from './api';
import { useApp } from './state';
import type { Key } from './i18n';
import catalog from '../src-tauri/catalog/packages.json';

export function Settings() {
  const { t, snapshot, preferences, busy, run, locale } = useApp();
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
              <Button
                onClick={() => void run(() => call('open_location', { location: 'workspace' }))}
              >
                <FolderOpen size={15} />
                {t('openWorkspace')}
              </Button>
            </div>
            {snapshot.root && <code className="workspace-path">{snapshot.root}</code>}
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
