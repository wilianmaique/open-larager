import { useEffect, useState } from 'react';
import { ArrowDownToLine, Check, Puzzle, RotateCw, Search } from 'lucide-react';
import { phpExtensionsApi } from './api';
import { Button, Empty, IconButton } from './components';
import { Select } from './controls';
import { useApp } from './state';
import type { PhpExtension, PhpExtensions as Inventory } from './types';

export function PhpExtensions({ version }: { version: string | null }) {
  const { t, busy, run, progress } = useApp();
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [refresh, setRefresh] = useState(0);
  const [pending, setPending] = useState<string | null>(null);
  useEffect(() => {
    if (!version) return;
    let active = true;
    setInventory(null);
    setLoading(true);
    setError('');
    void phpExtensionsApi
      .list()
      .then((data) => {
        if (active) setInventory(data);
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
  }, [version, refresh]);
  if (!version)
    return (
      <Empty
        icon={<Puzzle size={26} />}
        title={t('extensionsNeedPhp')}
        description={t('extensionsNeedPhpHelp')}
      />
    );
  const apply = async (extension: PhpExtension) => {
    if (!inventory) return;
    setPending(extension.id);
    const success = await run(
      async () => {
        const updated = await phpExtensionsApi.set(
          inventory.phpVersion,
          extension.id,
          !extension.loaded,
        );
        setInventory(updated);
      },
      t(extension.loaded ? 'extensionDisabled' : 'extensionEnabled', { name: extension.id }),
    );
    setPending(null);
    if (!success) setRefresh((value) => value + 1);
  };
  const entries =
    inventory?.extensions.filter(
      (extension) =>
        extension.id.toLowerCase().includes(query.toLowerCase().trim()) &&
        (filter === 'all' ||
          (filter === 'enabled' ? extension.loaded : !extension.loaded && extension.compatible)),
    ) ?? [];
  return (
    <section className="drawer-section php-extensions" aria-busy={loading || pending !== null}>
      <div className="section-mini-title">
        <Puzzle size={17} />
        <h3>{t('extensions')}</h3>
        <IconButton
          label={t('extensionsRefresh')}
          disabled={busy || loading}
          onClick={() => setRefresh((value) => value + 1)}
        >
          <RotateCw size={15} />
        </IconButton>
      </div>
      <p>{t('extensionsHelp')}</p>
      {inventory && (
        <div className="extension-runtime">
          <code>
            PHP {inventory.runtime.version} · {inventory.runtime.architecture} ·{' '}
            {inventory.runtime.threadSafe ? 'TS' : 'NTS'} ·{' '}
            {inventory.runtime.compiler.toUpperCase()}
          </code>
          <span className="tag">{t('extensionsAutomatic')}</span>
        </div>
      )}
      <p className="field-help">{t('extensionsReloadHelp')}</p>
      {loading && <p role="status">{t('extensionsLoading')}</p>}
      {error && (
        <div className="inline-error" role="alert">
          {error}
        </div>
      )}
      {inventory?.diagnostics && (
        <div className="inline-error" role="alert">
          <strong>{t('extensionsDiagnostics')}</strong>
          <pre>{inventory.diagnostics}</pre>
        </div>
      )}
      {inventory && (
        <>
          <div className="extension-toolbar">
            <label className="extension-search">
              <Search size={15} />
              <input
                type="search"
                aria-label={t('extensionsSearch')}
                placeholder={t('extensionsSearch')}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            <Select
              aria-label={t('extensionsFilter')}
              value={filter}
              onValueChange={setFilter}
              options={[
                { value: 'all', label: t('extensionsAll') },
                { value: 'enabled', label: t('extensionsEnabled') },
                { value: 'available', label: t('extensionsAvailable') },
              ]}
            />
          </div>
          <div className="extension-list">
            {entries.map((extension) => (
              <div
                className="extension-row"
                key={extension.id}
                data-testid={`extension-${extension.id}`}
              >
                <div className="extension-details">
                  <div>
                    <code>{extension.id}</code>
                    {extension.version && <small>{extension.version}</small>}
                  </div>
                  <span>
                    {t(
                      extension.builtin
                        ? 'extensionBuiltin'
                        : extension.source === 'bundled'
                          ? 'extensionBundled'
                          : 'extensionPecl',
                    )}
                  </span>
                  {extension.dependencies.length > 0 && (
                    <small>
                      {t('extensionDependencies', { names: extension.dependencies.join(', ') })}
                    </small>
                  )}
                  {!extension.compatible && (
                    <small>
                      {t('extensionUnavailable', { version: inventory.runtime.branch })}
                    </small>
                  )}
                </div>
                <div className="extension-action">
                  {extension.loaded && (
                    <span className="extension-loaded">
                      <Check size={13} />
                      {t('enabled')}
                    </span>
                  )}
                  {!extension.builtin && (
                    <Button
                      variant={extension.loaded ? 'default' : 'primary'}
                      disabled={
                        busy || pending !== null || (!extension.loaded && !extension.compatible)
                      }
                      onClick={() => void apply(extension)}
                    >
                      {pending === extension.id ? (
                        <RotateCw size={14} className="extension-working" />
                      ) : !extension.loaded && extension.source === 'pecl' ? (
                        <ArrowDownToLine size={14} />
                      ) : null}
                      {t(
                        pending === extension.id
                          ? 'extensionApplying'
                          : extension.loaded
                            ? 'extensionDisable'
                            : extension.source === 'pecl'
                              ? 'extensionInstall'
                              : 'extensionEnable',
                      )}
                    </Button>
                  )}
                </div>
              </div>
            ))}
            {entries.length === 0 && <p>{t('extensionsNoResults')}</p>}
          </div>
        </>
      )}
      {pending && progress?.service === 'php' && (
        <div className="extension-progress">
          <p role="status">
            {t(progress.phase)} · {pending}
          </p>
          {['downloading', 'verifying', 'extracting'].includes(progress.phase) && (
            <Button
              onClick={() =>
                void phpExtensionsApi.cancel().catch((reason: unknown) => setError(String(reason)))
              }
            >
              {t('cancel')}
            </Button>
          )}
        </div>
      )}
    </section>
  );
}
