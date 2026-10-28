import { lazy, Suspense, useEffect, useState } from 'react';
import { FileCode2, RotateCcw, Save } from 'lucide-react';
import { Button, Dialog, DialogHeader } from './components';
import { siteConfigApi } from './api';
import { useApp } from './state';
import type { Site, SiteConfig } from './types';

const ConfigEditor = lazy(() => import('./ConfigEditor'));

export function SiteConfigDialog({ site, onClose }: { site: Site; onClose: () => void }) {
  const { t, snapshot, busy, run } = useApp();
  const [config, setConfig] = useState<SiteConfig | null>(null);
  const [content, setContent] = useState('');
  const [original, setOriginal] = useState('');
  const [customized, setCustomized] = useState(false);
  const [reset, setReset] = useState(false);
  const [preview, setPreview] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [discard, setDiscard] = useState(false);
  const nginx = snapshot.services.find((service) => service.id === 'nginx')!;
  const dirty = content !== original || reset;
  const blocked = busy || nginx.status === 'running' || !nginx.version;
  const close = () => {
    if (dirty) setDiscard(true);
    else onClose();
  };
  useEffect(() => {
    let active = true;
    void siteConfigApi
      .read(site.id)
      .then((result) => {
        if (!active) return;
        setConfig(result);
        setContent(result.content);
        setOriginal(result.content);
        setCustomized(result.customized);
      })
      .catch((reason: unknown) => {
        if (active) setError(String(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [site.id]);

  return (
    <Dialog
      title={`${t('siteConfiguration')} ${site.hostname}`}
      onClose={close}
      className="site-config-dialog"
    >
      <DialogHeader title={site.hostname} subtitle={t('siteConfiguration')} onClose={close} />
      <div className="dialog-content">
        <p className="field-help">{t('siteConfigHelp')}</p>
        <div className="site-config-toolbar">
          <Button
            disabled={loading || busy || !config}
            onClick={() => {
              if (preview) {
                setPreview(false);
                return;
              }
              void run(async () => {
                setConfig(await siteConfigApi.preview(site.id, reset ? null : content));
                setPreview(true);
              });
            }}
          >
            <FileCode2 size={15} />
            {t(preview ? 'siteRules' : 'siteGeneratedConfig')}
          </Button>
          <Button
            disabled={loading || busy || !config || blocked}
            onClick={() =>
              void run(async () => {
                const result = await siteConfigApi.preview(site.id, null);
                setConfig(result);
                setContent(result.content);
                setReset(true);
                setPreview(false);
              })
            }
          >
            <RotateCcw size={15} />
            {t('siteResetRules')}
          </Button>
          <span className="tag">
            {t(reset || (!dirty && !customized) ? 'siteGenerated' : 'siteCustomized')}
          </span>
        </div>
        {error && <p role="alert">{error}</p>}
        {loading ? (
          <p role="status">{t('busy')}</p>
        ) : (
          config && (
            <div className="site-config-editor">
              <Suspense fallback={<p role="status">{t('busy')}</p>}>
                <ConfigEditor
                  value={preview ? config.generated : content}
                  readOnly={preview || blocked}
                  theme={document.documentElement.dataset.theme ?? 'light'}
                  onChange={(value) => {
                    // Monaco emits for controlled updates too, including previews.
                    if (preview || blocked || value === content) return;
                    setContent(value);
                    setReset(false);
                  }}
                />
              </Suspense>
            </div>
          )
        )}
        {config && <code className="site-config-path">{config.path}</code>}
        <p className="field-help">
          {t(
            !nginx.version
              ? 'siteInstallToValidate'
              : nginx.status === 'running'
                ? 'siteStopNginx'
                : 'siteValidationHelp',
          )}
        </p>
      </div>
      <div data-notification-host />
      <div className="dialog-actions">
        <Button onClick={close}>{t('cancel')}</Button>
        <Button
          variant="primary"
          disabled={blocked || loading || !config || !dirty}
          onClick={() =>
            void run(async () => {
              await siteConfigApi.save(site.id, reset ? null : content);
              const result = await siteConfigApi.read(site.id);
              setConfig(result);
              setContent(result.content);
              setOriginal(result.content);
              setCustomized(result.customized);
              setReset(false);
            }, t('siteConfigSaved'))
          }
        >
          <Save size={15} />
          {t('save')}
        </Button>
      </div>
      {discard && (
        <Dialog title={t('unsaved')} onClose={() => setDiscard(false)}>
          <DialogHeader title={t('unsaved')} onClose={() => setDiscard(false)} />
          <div className="dialog-actions">
            <Button onClick={() => setDiscard(false)}>{t('cancel')}</Button>
            <Button variant="danger" onClick={onClose}>
              {t('discard')}
            </Button>
          </div>
        </Dialog>
      )}
    </Dialog>
  );
}
