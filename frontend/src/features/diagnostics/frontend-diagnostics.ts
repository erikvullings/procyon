import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { FrontendDiagnostic } from '../../models';

type DiagnosticCode =
  | 'FRONTEND_UNCAUGHT_ERROR'
  | 'FRONTEND_UNHANDLED_REJECTION'
  | 'FRONTEND_CONSOLE_ERROR';

export interface FrontendDiagnosticsReporter {
  readonly flush: () => void;
  readonly uninstall: () => void;
}

/** Installs release-safe frontend error reporting. HTTP startup can defer until authentication. */
export function installFrontendDiagnostics(
  client: FileManagerClient,
  options: { readonly deferUntilReady?: boolean } = {},
): FrontendDiagnosticsReporter {
  const originalConsoleError = console.error;
  let suppressConsoleCapture = false;
  let ready = options.deferUntilReady !== true;
  const pending: FrontendDiagnostic[] = [];

  const submit = (error: FrontendDiagnostic) => {
    if (!ready) {
      pending.push(error);
      if (pending.length > 50) pending.shift();
      return;
    }
    void client.recordFrontendDiagnostic(error).catch((failure: unknown) => {
      ready = false;
      pending.push(error);
      if (pending.length > 50) pending.shift();
      suppressConsoleCapture = true;
      try {
        originalConsoleError('Unable to record frontend diagnostic', failure);
      } finally {
        suppressConsoleCapture = false;
      }
    });
  };
  const report = (code: DiagnosticCode, values: readonly unknown[], context?: string) => {
    submit({
      timestamp: new Date().toISOString(),
      code,
      message: values.map(formatValue).join(' '),
      ...(context === undefined ? {} : { context }),
    });
  };

  const onError = (event: ErrorEvent) => {
    const error = event.error instanceof Error ? event.error : undefined;
    report('FRONTEND_UNCAUGHT_ERROR', [error ?? event.message], error?.stack);
  };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => {
    const error = event.reason instanceof Error ? event.reason : undefined;
    report('FRONTEND_UNHANDLED_REJECTION', [error ?? event.reason], error?.stack);
  };

  console.error = (...values: unknown[]) => {
    originalConsoleError(...values);
    if (!suppressConsoleCapture) {
      const error = values.find((value): value is Error => value instanceof Error);
      report('FRONTEND_CONSOLE_ERROR', values, error?.stack);
    }
  };
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onUnhandledRejection);

  return {
    flush: () => {
      ready = true;
      const queued = pending.splice(0);
      for (const error of queued) submit(error);
    },
    uninstall: () => {
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onUnhandledRejection);
      console.error = originalConsoleError;
    },
  };
}

function formatValue(value: unknown): string {
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}
