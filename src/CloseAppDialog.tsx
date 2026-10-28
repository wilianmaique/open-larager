import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { LogOut, PanelBottomClose } from 'lucide-react';
import { desktopApi, native } from './api';
import { Button, Dialog, DialogHeader } from './components';
import { Checkbox } from './controls';
import { useApp } from './state';
import type { CloseAction } from './types';

export function CloseAppDialog() {
  const { t, connected, refresh } = useApp();
  const [open, setOpen] = useState(false);
  const [remember, setRemember] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(false);
  const [trayReady, setTrayReady] = useState(false);
  const resolving = useRef(false);

  useEffect(() => {
    if (!native) return;
    let disposed = false;
    const subscription = listen('app-close-requested', () => {
      if (!disposed) setOpen(true);
    });
    // Query after subscribing so a close during startup cannot be lost.
    void subscription
      .then(() => desktopApi.pendingClose())
      .then((value) => {
        if (!disposed && value) setOpen(true);
      })
      .catch(() => {
        if (!disposed) setError(true);
      });
    return () => {
      disposed = true;
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!native || !connected) return;
    let disposed = false;
    void desktopApi
      .configureTray({ open: t('trayOpen'), exit: t('exitApp') })
      .then(() => {
        if (!disposed) setTrayReady(true);
      })
      .catch(() => {
        if (!disposed) setTrayReady(false);
      });
    return () => {
      disposed = true;
    };
  }, [t, connected]);

  const resolve = async (action: CloseAction | null) => {
    if (resolving.current) return;
    resolving.current = true;
    setPending(true);
    setError(false);
    try {
      await desktopApi.resolveClose(action, action !== null && remember);
      setOpen(false);
      setRemember(false);
      if (action !== 'exit') await refresh();
    } catch {
      setError(true);
    } finally {
      resolving.current = false;
      setPending(false);
    }
  };

  if (!open) return null;
  return (
    <Dialog
      title={t('closeAppTitle')}
      className="close-app-dialog"
      onClose={() => {
        if (!resolving.current) void resolve(null);
      }}
    >
      <DialogHeader
        title={t('closeAppTitle')}
        onClose={() => {
          if (!resolving.current) void resolve(null);
        }}
      />
      <div className="dialog-content">
        <p>{t('closeAppDescription')}</p>
        {!trayReady && <p role="status">{t('trayUnavailable')}</p>}
        <Checkbox
          checked={remember}
          disabled={pending}
          onChange={(event) => setRemember(event.target.checked)}
        >
          {t('rememberCloseChoice')}
        </Checkbox>
        <p className="field-help">{t('closeChoiceHelp')}</p>
        {error && <p role="alert">{t('closeAppError')}</p>}
      </div>
      <div className="dialog-actions">
        <Button disabled={pending} onClick={() => void resolve(null)}>
          {t('cancel')}
        </Button>
        <Button disabled={pending} onClick={() => void resolve('exit')}>
          <LogOut size={16} />
          {t('exitApp')}
        </Button>
        <Button
          variant="primary"
          disabled={pending || !trayReady}
          onClick={() => void resolve('tray')}
        >
          <PanelBottomClose size={16} />
          {t('minimizeToTray')}
        </Button>
      </div>
    </Dialog>
  );
}
