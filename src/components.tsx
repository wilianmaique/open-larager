import { useEffect, useRef, type ButtonHTMLAttributes, type ReactNode } from 'react';
import {
  X,
  LoaderCircle,
  Terminal,
  Feather,
  Database,
  Layers3,
  Mail,
  LockKeyhole,
} from 'lucide-react';
import { services } from './serviceRegistry';
import { useApp } from './state';
import type { ServiceId, ServiceStatus } from './types';

export function Button({
  children,
  variant = 'default',
  loading = false,
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'default' | 'primary' | 'ghost' | 'danger';
  loading?: boolean;
}) {
  return (
    <button
      className={`button button-${variant} ${className}`}
      {...props}
      disabled={props.disabled || loading}
    >
      {loading && <LoaderCircle size={15} className="spin" />}
      {children}
    </button>
  );
}
export function IconButton({
  children,
  label,
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button className={`icon-button ${className}`} aria-label={label} title={label} {...props}>
      {children}
    </button>
  );
}
export function ServiceIcon({ id, large = false }: { id: ServiceId; large?: boolean }) {
  const data = services[id];
  const icons = {
    apache: Feather,
    mysql: Database,
    postgresql: Database,
    redis: Layers3,
    mailpit: Mail,
    caddy: LockKeyhole,
  };
  const Icon = id in icons ? icons[id as keyof typeof icons] : null;
  return (
    <span
      className={`service-icon service-icon-${id} ${large ? 'service-icon-large' : ''}`}
      style={{ '--service-color': data.color } as React.CSSProperties}
    >
      {Icon ? (
        <Icon size={large ? 28 : 22} strokeWidth={id === 'apache' ? 1.8 : 1.7} />
      ) : (
        <span>{data.mark}</span>
      )}
    </span>
  );
}
export function Status({ status }: { status: ServiceStatus['status'] }) {
  const { t } = useApp();
  return (
    <span className={`status status-${status}`}>
      <span />
      {t(status)}
    </span>
  );
}
export function Empty({
  icon,
  title,
  description,
  children,
}: {
  icon?: ReactNode;
  title: string;
  description?: string;
  children?: ReactNode;
}) {
  return (
    <div className="empty">
      <div className="empty-icon">{icon ?? <Terminal size={26} />}</div>
      <h3>{title}</h3>
      {description && <p>{description}</p>}
      {children}
    </div>
  );
}
export function Dialog({
  title,
  children,
  onClose,
  className = '',
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  className?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const element = ref.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className={`dialog ${className}`}
      aria-label={title}
      onCancel={(event) => {
        event.preventDefault();
        closeRef.current();
      }}
      onClick={(event) => {
        if (event.target === ref.current) {
          const bounds = ref.current.getBoundingClientRect();
          if (
            event.clientX < bounds.left ||
            event.clientX > bounds.right ||
            event.clientY < bounds.top ||
            event.clientY > bounds.bottom
          )
            closeRef.current();
        }
      }}
    >
      {children}
    </dialog>
  );
}
export function DialogHeader({
  title,
  subtitle,
  onClose,
}: {
  title: string;
  subtitle?: string;
  onClose: () => void;
}) {
  const { t } = useApp();
  return (
    <header className="dialog-header">
      <div>
        <h2>{title}</h2>
        {subtitle && <p>{subtitle}</p>}
      </div>
      <IconButton label={t('close')} onClick={onClose}>
        <X size={20} />
      </IconButton>
    </header>
  );
}
export function formatBytes(bytes: number, locale: string): string {
  return (
    new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(bytes / 1024 / 1024) + ' MB'
  );
}
