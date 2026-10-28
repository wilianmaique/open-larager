import type { FileFailure } from './types';
import type { Key, translator } from './i18n';

const operations: Record<FileFailure['operation'], Key> = {
  inspect: 'fileOpInspect',
  read: 'fileOpRead',
  create: 'fileOpCreate',
  write: 'fileOpWrite',
  copy: 'fileOpCopy',
  verify: 'fileOpVerify',
  remove: 'fileOpRemove',
  lock: 'fileOpLock',
  move: 'fileOpMove',
  permissions: 'fileOpPermissions',
};

export function isFileFailure(value: unknown): value is FileFailure {
  if (!value || typeof value !== 'object') return false;
  const issue = value as Partial<FileFailure>;
  return (
    issue.kind === 'fileOperation' &&
    typeof issue.path === 'string' &&
    typeof issue.message === 'string' &&
    typeof issue.operation === 'string' &&
    Object.hasOwn(operations, issue.operation) &&
    Array.isArray(issue.processes)
  );
}

export function FileFailureDetails({
  issue,
  t,
}: {
  issue: FileFailure;
  t: ReturnType<typeof translator>;
}) {
  return (
    <div className="file-failure" aria-label={t('fileDiagnosticTitle')}>
      <p>{issue.message}</p>
      <dl>
        <div>
          <dt>{t('fileOperationLabel')}</dt>
          <dd>{t(operations[issue.operation])}</dd>
        </div>
        <div>
          <dt>{t('filePathLabel')}</dt>
          <dd>
            <code>{issue.path}</code>
          </dd>
        </div>
        {issue.relatedPath && (
          <div>
            <dt>{t('fileRelatedPathLabel')}</dt>
            <dd>
              <code>{issue.relatedPath}</code>
            </dd>
          </div>
        )}
        {issue.osCode !== null && (
          <div>
            <dt>{t('fileSystemCode')}</dt>
            <dd>{issue.osCode}</dd>
          </div>
        )}
      </dl>
      {issue.processes.length > 0 && (
        <>
          <h4>{t('fileProcessesLabel')}</h4>
          <ul>
            {issue.processes.map((process) => (
              <li key={process.pid}>
                <span>
                  {process.name || t('fileUnknownProcess')} ·{' '}
                  {t('fileProcessPid', { pid: process.pid })}
                </span>
                {process.executable && <code>{process.executable}</code>}
                {process.currentProcess && <p>{t('fileCurrentProcess')}</p>}
              </li>
            ))}
          </ul>
          <p>{t('fileCloseProcessHelp')}</p>
        </>
      )}
      {issue.lockCheck === 'notIdentified' && <p>{t('fileProcessNotIdentified')}</p>}
      {issue.lockCheck === 'unavailable' && <p>{t('fileProcessUnavailable')}</p>}
      {issue.diagnosticCode !== null && (
        <p>{t('fileDiagnosticCode', { code: issue.diagnosticCode })}</p>
      )}
    </div>
  );
}
