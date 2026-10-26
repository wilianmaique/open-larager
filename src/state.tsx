import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { listen } from '@tauri-apps/api/event';
import { createPortal } from 'react-dom';
import { call, emptySnapshot, getSnapshot, native } from './api';
import { detectLocale, translator, type Key } from './i18n';
import type { Locale, Page, Progress, ServiceId, Snapshot } from './types';

interface Context {
  snapshot: Snapshot;
  page: Page;
  setPage: (page: Page) => void;
  selected: ServiceId | null;
  setSelected: (id: ServiceId | null) => void;
  locale: Locale;
  t: ReturnType<typeof translator>;
  busy: boolean;
  progress: Progress | null;
  run: (work: () => Promise<unknown>, success?: string) => Promise<boolean>;
  refresh: () => Promise<void>;
  preferences: (language: string, theme: string) => Promise<void>;
  toast: (message: string) => void;
  connected: boolean;
}
const AppContext = createContext<Context | null>(null);
export const useApp = () => {
  const context = useContext(AppContext);
  if (!context) throw new Error('App provider missing');
  return context;
};

export function AppProvider({ children }: { children: ReactNode }) {
  const [snapshot, setSnapshot] = useState<Snapshot>(emptySnapshot);
  const [page, setPage] = useState<Page>('overview');
  const [selected, setSelected] = useState<ServiceId | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [notification, setNotification] = useState<{ message: string; error: boolean } | null>(
    null,
  );
  const [connected, setConnected] = useState(!native);
  const locale =
    snapshot.settings.language === 'auto'
      ? detectLocale(snapshot.systemLocale)
      : (snapshot.settings.language as Locale);
  const t = useMemo(() => translator(locale), [locale]);
  const refresh = useCallback(async () => {
    try {
      const state = await getSnapshot();
      setSnapshot(state);
      setConnected(true);
    } catch (error) {
      setConnected(false);
      setNotification({ message: String(error), error: true });
    }
  }, []);
  useEffect(() => {
    void refresh();
    if (!native) return;
    const id = setInterval(() => {
      void refresh();
    }, 3000);
    return () => clearInterval(id);
  }, [refresh]);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    void listen<Progress>('install-progress', (event) => setProgress(event.payload)).then(
      (unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      },
    );
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  useEffect(() => {
    if (!notification || notification.error) return;
    const timer = setTimeout(() => setNotification(null), 4500);
    return () => clearTimeout(timer);
  }, [notification]);
  useEffect(() => {
    document.documentElement.lang = locale;
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => {
      document.documentElement.dataset.theme =
        snapshot.settings.theme === 'system'
          ? media.matches
            ? 'dark'
            : 'light'
          : snapshot.settings.theme;
    };
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, [locale, snapshot.settings.theme]);
  const run = async (work: () => Promise<unknown>, success?: string) => {
    if (busy) return false;
    setBusy(true);
    setNotification(null);
    try {
      await work();
      if (success) setNotification({ message: success, error: false });
      await refresh();
      return true;
    } catch (error) {
      setNotification({
        message: String(error).includes('DESKTOP_REQUIRED') ? t('desktopRequired') : String(error),
        error: true,
      });
      return false;
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };
  const preferences = async (language: string, theme: string) => {
    if (!native) {
      setSnapshot((current) => ({
        ...current,
        settings: { ...current.settings, language, theme },
      }));
      return;
    }
    await run(() => call('save_preferences', { language, theme }), t('saved'));
  };
  return (
    <AppContext.Provider
      value={{
        snapshot,
        page,
        setPage,
        selected,
        setSelected,
        locale,
        t,
        busy,
        progress,
        run,
        refresh,
        preferences,
        connected,
        toast: (message) => setNotification({ message, error: false }),
      }}
    >
      {children}
      {notification &&
        createPortal(
          <div
            className={`notification ${notification.error ? 'notification-error' : ''}`}
            role={notification.error ? 'alert' : 'status'}
          >
            <span className="notification-dot" />
            <div>
              {notification.error && <strong>{t('operationFailed')}</strong>}
              <p>{notification.message}</p>
            </div>
            <button aria-label={t('dismiss')} onClick={() => setNotification(null)}>
              ×
            </button>
          </div>,
          Array.from(document.querySelectorAll('dialog[open]')).at(-1) ?? document.body,
        )}
    </AppContext.Provider>
  );
}

export const descriptionKeys: Record<ServiceId, Key> = {
  php: 'descriptionPhp',
  apache: 'descriptionApache',
  mysql: 'descriptionMysql',
  postgresql: 'descriptionPostgresql',
  nginx: 'descriptionNginx',
  redis: 'descriptionRedis',
  mailpit: 'descriptionMailpit',
  caddy: 'descriptionCaddy',
};
