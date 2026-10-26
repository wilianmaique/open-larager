import { useEffect, useState } from 'react';
import { FolderOpen, RefreshCw, ScrollText } from 'lucide-react';
import { Button, Empty } from './components';
import { Checkbox, Select } from './controls';
import { call } from './api';
import { useApp } from './state';
import { services } from './serviceRegistry';
import type { ServiceId } from './types';

export function LogsPage() {
  const { t, snapshot } = useApp();
  const installed = snapshot.services.filter((s) => s.version);
  const [selected, setSelected] = useState<ServiceId | ''>(installed[0]?.id ?? '');
  return (
    <>
      <div className="page-heading">
        <div>
          <span className="section-eyebrow">{t('tools')}</span>
          <h1>{t('nav.logs')}</h1>
          <p>{t('logsDescription')}</p>
        </div>
        <Select<ServiceId | ''>
          className="log-select"
          aria-label={t('selectService')}
          value={selected}
          onValueChange={setSelected}
          placeholder={t('selectService')}
          options={installed.map((s) => ({ value: s.id, label: services[s.id].name }))}
        />
      </div>
      <section className="logs-page-panel">
        {selected ? (
          <Logs id={selected} />
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
export function Logs({ id }: { id: ServiceId }) {
  const { t, snapshot, run } = useApp();
  const [content, setContent] = useState('');
  const [live, setLive] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const installed = Boolean(snapshot.settings.services[id].activeVersion);
  const refresh = async () => {
    if (!installed) return;
    setLoading(true);
    try {
      setContent(await call<string>('read_logs', { id }));
      setError('');
    } catch (error) {
      setError(String(error));
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => {
    let active = true;
    const load = async () => {
      if (!active) return;
      if (!installed) {
        setContent('');
        return;
      }
      try {
        const result = await call<string>('read_logs', { id });
        if (active) {
          setContent(result);
          setError('');
        }
      } catch (error) {
        if (active) setError(String(error));
      }
    };
    void load();
    const interval = live ? setInterval(() => void load(), 3000) : undefined;
    return () => {
      active = false;
      clearInterval(interval);
    };
  }, [id, live, installed]);
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
          <Button loading={loading} onClick={() => void refresh()}>
            <RefreshCw size={14} />
            {t('refresh')}
          </Button>
          <Button onClick={() => void run(() => call('open_location', { id, location: 'logs' }))}>
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
