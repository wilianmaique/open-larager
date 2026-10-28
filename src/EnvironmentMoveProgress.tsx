import type { EnvironmentMoveProgress as MoveProgress, Locale } from './types';
import type { translator, Key } from './i18n';

const phaseKeys: Record<MoveProgress['phase'], Key> = {
  scanning: 'environmentScanning',
  copying: 'environmentCopying',
  switching: 'environmentSwitching',
  cleaning: 'environmentCleaning',
  complete: 'environmentComplete',
};

function size(bytes: number, locale: Locale) {
  const units = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte'];
  const index = Math.min(units.length - 1, Math.floor(Math.log10(Math.max(1, bytes)) / 3));
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: units[index],
    unitDisplay: 'short',
    maximumFractionDigits: index ? 1 : 0,
  }).format(bytes / 1000 ** index);
}

function duration(seconds: number, locale: Locale) {
  const format = (value: number, unit: string) =>
    new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay: 'short' }).format(value);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  if (hours)
    return [format(hours, 'hour'), minutes ? format(minutes, 'minute') : ''].join(' ').trim();
  if (minutes)
    return [format(minutes, 'minute'), remainder ? format(remainder, 'second') : '']
      .join(' ')
      .trim();
  return format(Math.max(1, remainder), 'second');
}

export function EnvironmentMoveProgress({
  progress,
  locale,
  t,
}: {
  progress: MoveProgress | null;
  locale: Locale;
  t: ReturnType<typeof translator>;
}) {
  const phase = progress?.phase ?? 'scanning';
  const percentage =
    phase === 'complete'
      ? 100
      : progress && phase !== 'scanning' && progress.totalProcessingBytes > 0
        ? Math.min(99, Math.floor((progress.processedBytes / progress.totalProcessingBytes) * 100))
        : undefined;
  const estimating = phase === 'copying' || phase === 'cleaning';
  return (
    <div className="environment-move-progress">
      <div className="environment-progress-heading" role="status">
        <strong>{t(phaseKeys[phase])}</strong>
        {percentage !== undefined && (
          <span>
            {new Intl.NumberFormat(locale, { style: 'percent' }).format(percentage / 100)}
          </span>
        )}
      </div>
      <progress aria-label={t('environmentProgress')} max={100} value={percentage} />
      {progress && phase !== 'scanning' && (
        <div className="environment-progress-details">
          <span>
            {t('environmentBytesCopied', {
              copied: size(progress.copiedBytes, locale),
              total: size(progress.totalBytes, locale),
            })}
          </span>
          <span>
            {t('environmentFilesCopied', {
              copied: new Intl.NumberFormat(locale).format(progress.copiedFiles),
              total: new Intl.NumberFormat(locale).format(progress.totalFiles),
            })}
          </span>
        </div>
      )}
      {estimating && (
        <p>
          {progress?.estimatedSecondsRemaining != null
            ? t('environmentTimeRemaining', {
                time: duration(progress.estimatedSecondsRemaining, locale),
              })
            : t(
                progress && progress.processedBytes >= progress.totalProcessingBytes
                  ? 'environmentFinishing'
                  : 'environmentEstimating',
              )}
        </p>
      )}
      <small>{t('environmentProgressHelp')}</small>
    </div>
  );
}
