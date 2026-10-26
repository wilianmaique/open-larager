import { useEffect, useState } from 'react';
import { AnimatePresence, motion, MotionConfig } from 'motion/react';
import {
  LayoutDashboard,
  Server,
  Globe2,
  ScrollText,
  Settings2,
  Search,
  ChevronRight,
  ArrowUpRight,
  ShieldCheck,
  CircleHelp,
  Menu,
  X,
  FolderOpen,
  Monitor,
  Command,
  ArrowRight,
} from 'lucide-react';
import { useApp } from './state';
import { native, call } from './api';
import { Button, Dialog, Empty, IconButton, ServiceIcon } from './components';
import { services } from './serviceRegistry';
import { serviceIds, type Page } from './types';
import type { Key } from './i18n';
import { Overview, ServicesPage } from './Services';
import { ServiceDrawer } from './ServiceDrawer';
import { Domains } from './Domains';
import { LogsPage } from './Logs';
import { Settings } from './Settings';

const pages = ['overview', 'services', 'domains', 'logs', 'settings'] as const;
const pageIcons = {
  overview: LayoutDashboard,
  services: Server,
  domains: Globe2,
  logs: ScrollText,
  settings: Settings2,
};

export default function App() {
  const { t, page, setPage, selected, setSelected, snapshot, run, connected, refresh } = useApp();
  const [searchOpen, setSearchOpen] = useState(false);
  const [mobileNav, setMobileNav] = useState(false);
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setSearchOpen((value) => !value);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);
  const navigate = (next: Page) => {
    setPage(next);
    setMobileNav(false);
  };
  return (
    <MotionConfig reducedMotion="user">
      <div className="app-shell">
        {mobileNav && (
          <button
            className="nav-backdrop"
            aria-label={t('close')}
            onClick={() => setMobileNav(false)}
          />
        )}
        <aside className={`sidebar ${mobileNav ? 'sidebar-open' : ''}`}>
          <a
            className="brand"
            href="#"
            onClick={(event) => {
              event.preventDefault();
              navigate('overview');
            }}
          >
            <img src="/brand.svg" alt="" />
            <span>
              open<span className="brand-weight">larager</span>
              <small>{t('localEnvironment')}</small>
            </span>
          </a>
          <div className="nav-label">{t('environment')}</div>
          <nav aria-label={t('environment')}>
            {pages.slice(0, 3).map((id) => {
              const Icon = pageIcons[id];
              return (
                <button
                  key={id}
                  aria-label={t(`nav.${id}`)}
                  className={`nav-item ${page === id ? 'active' : ''}`}
                  onClick={() => navigate(id)}
                  aria-current={page === id ? 'page' : undefined}
                >
                  <Icon size={19} />
                  <span>{t(`nav.${id}`)}</span>
                  {id === 'services' && (
                    <span aria-hidden="true" className="nav-count">
                      {snapshot.services.filter((s) => s.version).length}
                    </span>
                  )}
                </button>
              );
            })}
          </nav>
          <div className="nav-label nav-label-tools">{t('tools')}</div>
          <nav aria-label={t('tools')}>
            {pages.slice(3).map((id) => {
              const Icon = pageIcons[id];
              return (
                <button
                  key={id}
                  className={`nav-item ${page === id ? 'active' : ''}`}
                  onClick={() => navigate(id)}
                  aria-current={page === id ? 'page' : undefined}
                >
                  <Icon size={19} />
                  <span>{t(`nav.${id}`)}</span>
                </button>
              );
            })}
            <button
              className="nav-item"
              onClick={() => void run(() => call('open_location', { location: 'projects' }))}
            >
              <FolderOpen size={19} />
              <span>{t('projectFolder')}</span>
              <ArrowUpRight size={14} className="nav-external" />
            </button>
          </nav>
          <div className="sidebar-bottom">
            <button className="sidebar-footer" onClick={() => navigate('settings')}>
              <span className="version-dot" />
              Open Larager <span className="app-version">v0.1.0</span>
              <CircleHelp size={16} />
            </button>
          </div>
        </aside>
        <div className="main-shell">
          <header className="topbar">
            <div className="breadcrumbs">
              <IconButton
                label={mobileNav ? t('close') : t('environment')}
                className="mobile-menu"
                onClick={() => setMobileNav((value) => !value)}
              >
                {mobileNav ? <X size={20} /> : <Menu size={20} />}
              </IconButton>
              <span>{t('workspace')}</span>
              <ChevronRight size={14} />
              <strong>{t(`nav.${page}`)}</strong>
            </div>
            <button className="search-trigger" onClick={() => setSearchOpen(true)}>
              <Search size={16} />
              <span>{t('search')}</span>
              <kbd>Ctrl K</kbd>
            </button>
            <div className={`connection ${!connected ? 'connection-error' : ''}`}>
              <span />
              <span>
                {!connected
                  ? t('connectionFailed')
                  : native
                    ? t('nativeHealth')
                    : t('browserPreview')}
              </span>
            </div>
          </header>
          <main id="main-content">
            {!native && (
              <div className="preview-banner">
                <Monitor size={15} />
                {t('desktopRequired')}
              </div>
            )}
            {!connected && (
              <div className="connection-banner" role="alert">
                {t('connectionFailed')}
                <Button onClick={() => void refresh()}>{t('connectionRetry')}</Button>
              </div>
            )}
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={page}
                className="page"
                initial={{ opacity: 0, y: 7 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -4 }}
                transition={{ duration: 0.16 }}
              >
                {page === 'overview' && <Overview />}
                {page === 'services' && <ServicesPage />}
                {page === 'domains' && <Domains />}
                {page === 'logs' && <LogsPage />}
                {page === 'settings' && <Settings />}
              </motion.div>
            </AnimatePresence>
            <footer className="main-footer">
              <span>
                <ShieldCheck size={13} />
                {t('localOnly')}
              </span>
              <span>
                Open Larager <span className="footer-separator">/</span> {t('localEnvironment')}
              </span>
            </footer>
          </main>
        </div>
      </div>
      {selected && <ServiceDrawer key={selected} id={selected} onClose={() => setSelected(null)} />}
      {searchOpen && <SearchDialog onClose={() => setSearchOpen(false)} />}
    </MotionConfig>
  );
}

function SearchDialog({ onClose }: { onClose: () => void }) {
  const { t, setPage, setSelected } = useApp();
  const [query, setQuery] = useState('');
  const matches = serviceIds.filter((id) =>
    services[id].name.toLowerCase().includes(query.toLowerCase()),
  );
  const pageMatches = pages.filter((id) =>
    t(`nav.${id}` as Key)
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  return (
    <Dialog title={t('search')} onClose={onClose} className="search-dialog">
      <div className="palette-input">
        <Search size={20} />
        <input
          autoFocus
          aria-label={t('search')}
          placeholder={t('searchPlaceholder')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <kbd>Esc</kbd>
      </div>
      <div className="palette-results">
        {matches.length > 0 && <span className="section-eyebrow">{t('nav.services')}</span>}
        {matches.map((id) => (
          <button
            key={id}
            onClick={() => {
              setSelected(id);
              onClose();
            }}
          >
            <ServiceIcon id={id} />
            <span>{services[id].name}</span>
            <ArrowUpRight size={16} />
          </button>
        ))}
        {pageMatches.length > 0 && <span className="section-eyebrow">{t('workspace')}</span>}
        {pageMatches.map((id) => {
          const Icon = pageIcons[id];
          return (
            <button
              key={id}
              onClick={() => {
                setPage(id);
                onClose();
              }}
            >
              <Icon size={20} />
              <span>{t(`nav.${id}`)}</span>
              <ArrowRight size={16} />
            </button>
          );
        })}
        {matches.length + pageMatches.length === 0 && (
          <Empty icon={<Search size={24} />} title={t('noResults')} />
        )}
      </div>
      <div className="palette-footer">
        <Command size={12} />
        <span>Ctrl K</span>
        <span>{t('keyboardHint')}</span>
      </div>
    </Dialog>
  );
}
