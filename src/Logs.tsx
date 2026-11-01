import { useEffect, useRef, useState } from 'react';
import { FolderOpen, RefreshCw, ScrollText, Trash2 } from 'lucide-react';
import { Button, Dialog, DialogHeader, Empty } from './components';
import { Checkbox, Select } from './controls';
import { call, logsApi } from './api';
import { useApp } from './state';
import { services } from './serviceRegistry';
import type { ServiceId } from './types';

export function LogsPage() {
  const { t, snapshot } = useApp();
  const installed = snapshot.services.filter((s) => s.version && services[s.id].daemon);
  const [selected, setSelected] = useState<ServiceId | ''>(installed[0]?.id ?? '');
  const [revision, setRevision] = useState(0);
  const current = installed.some((service) => service.id === selected)
    ? selected
    : (installed[0]?.id ?? '');
  return (
    <>
      <div className="page-heading">
        <div>
          <span className="section-eyebrow">{t('tools')}</span>
          <h1>{t('nav.logs')}</h1>
          <p>{t('logsDescription')}</p>
        </div>
        <div className="logs-page-actions">
          <Select<ServiceId | ''>
            className="log-select"
            aria-label={t('selectService')}
            value={current}
            onValueChange={setSelected}
            placeholder={t('selectService')}
            options={installed.map((s) => ({ value: s.id, label: services[s.id].name }))}
          />
          <ClearLogsButton onCleared={() => setRevision((value) => value + 1)} />
        </div>
      </div>
      <section className="logs-page-panel">
        {current ? (
          <Logs key={current} id={current} refreshToken={revision} />
        ) : (
          <Empty
            icon={<ScrollText size={28} />}
            title={t('noLogs')}
            description={t('noLogsDescription')}
          />
        )}
      </section>
    </>
  );
}
function ClearLogsButton({ id, onCleared }: { id?: ServiceId; onCleared: () => void }) {
  const { t, busy, run } = useApp();
  const [confirm, setConfirm] = useState(false);
  const title = id ? t('clearServiceLogs', { name: services[id].name }) : t('clearAllLogs');
  return (
    <>
      <Button disabled={busy} onClick={() => setConfirm(true)}>
        <Trash2 size={14} />
        {id ? t('clearLogs') : t('clearAllLogs')}
      </Button>
      {confirm && (
        <Dialog
          title={title}
          onClose={() => {
            if (!busy) setConfirm(false);
          }}
          className="clear-logs-dialog"
        >
          <DialogHeader
            title={title}
            onClose={() => {
              if (!busy) setConfirm(false);
            }}
          />
          <div className="dialog-content">
            <div data-notification-host />
            <p>
              {id ? t('clearServiceLogsHelp', { name: services[id].name }) : t('clearAllLogsHelp')}
            </p>
            <p>{t('clearLogsRunningHelp')}</p>
          </div>
          <div className="dialog-actions">
            <Button disabled={busy} onClick={() => setConfirm(false)}>
              {t('cancel')}
            </Button>
            <Button
              variant="danger"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await logsApi.clear(id ?? null);
                  onCleared();
                  setConfirm(false);
                }, t('logsCleared'))
              }
            >
              <Trash2 size={14} />
              {t('clearLogsConfirm')}
            </Button>
          </div>
        </Dialog>
      )}
    </>
  );
}

export function Logs({ id, refreshToken = 0 }: { id: ServiceId; refreshToken?: number }) {
  const { t, snapshot, run, busy } = useApp();
  const [content, setContent] = useState('');
  const [live, setLive] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const request = useRef(0);
  const manualRequest = useRef(0);
  const version = snapshot.settings.services[id].activeVersion;
  const installed = Boolean(version);
  const refresh = async () => {
    if (!installed) return;
    const token = ++request.current;
    const manualToken = ++manualRequest.current;
    setLoading(true);
    try {
      const result = await logsApi.read(id);
      if (token === request.current) {
        setContent(result);
        setError('');
      }
    } catch (error) {
      if (token === request.current) setError(String(error));
    } finally {
      if (manualToken === manualRequest.current) setLoading(false);
    }
  };
  useEffect(() => {
    let active = true;
    ++request.current;
    ++manualRequest.current;
    setLoading(false);
    const load = async () => {
      if (!active || busy) return;
      if (!installed) {
        setContent('');
        return;
      }
      const token = ++request.current;
      try {
        const result = await logsApi.read(id);
        if (active && token === request.current) {
          setContent(result);
          setError('');
        }
      } catch (error) {
        if (active && token === request.current) setError(String(error));
      }
    };
    void load();
    const interval = live ? setInterval(() => void load(), 3000) : undefined;
    return () => {
      active = false;
      clearInterval(interval);
    };
  }, [id, live, version, busy, revision, refreshToken]);
  if (!installed)
    return (
      <Empty
        icon={<ScrollText size={26} />}
        title={t('noLogs')}
        description={t('noLogsDescription')}
      />
    );
  return (
    <div className="logs-view">
      <div className="logs-toolbar">
        <span className="log-file">
          <span className={`live-dot ${live ? 'live' : ''}`} />
          <code>{services[id].name}</code>
        </span>
        <div>
          <Checkbox checked={live} onChange={(e) => setLive(e.target.checked)}>
            {t('autoRefresh')}
          </Checkbox>
          <Button disabled={busy} loading={loading} onClick={() => void refresh()}>
            <RefreshCw size={14} />
            {t('refresh')}
          </Button>
          <ClearLogsButton
            id={id}
            onCleared={() => {
              ++request.current;
              setContent('');
              setError('');
              setRevision((value) => value + 1);
            }}
          />
          <Button
            aria-label={t('openLogsFolder')}
            disabled={busy}
            onClick={() => void run(() => call('open_location', { id, location: 'logs' }))}
          >
            <FolderOpen size={14} />
          </Button>
        </div>
      </div>
      {error && (
        <div className="inline-error" role="alert">
          {error}
        </div>
      )}
      {content.trim() ? (
        <pre className="log-content" tabIndex={0} aria-label={t('logs')}>
          {content}
        </pre>
      ) : (
        <Empty
          icon={<ScrollText size={26} />}
          title={t('noLogs')}
          description={t('noLogsDescription')}
        />
      )}
    </div>
  );
}
