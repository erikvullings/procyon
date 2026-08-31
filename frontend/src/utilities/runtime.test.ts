import { describe, expect, it } from 'vitest';

import {
  DEFAULT_RUNTIME_KIND,
  RUNTIME_KINDS,
  RuntimeConfigurationError,
  resolveRuntimeKind,
} from './runtime';

describe('resolveRuntimeKind', () => {
  it('accepts every runtime the specification defines', () => {
    expect(RUNTIME_KINDS).toEqual(['http', 'tauri', 'mock']);

    for (const kind of RUNTIME_KINDS) {
      expect(resolveRuntimeKind(kind)).toBe(kind);
    }
  });

  it('falls back to the default when the variable is unset or blank', () => {
    expect(DEFAULT_RUNTIME_KIND).toBe('http');
    expect(resolveRuntimeKind(undefined)).toBe(DEFAULT_RUNTIME_KIND);
    expect(resolveRuntimeKind('')).toBe(DEFAULT_RUNTIME_KIND);
    expect(resolveRuntimeKind('   ')).toBe(DEFAULT_RUNTIME_KIND);
  });

  it('tolerates surrounding whitespace and casing from shell quoting', () => {
    expect(resolveRuntimeKind('  mock  ')).toBe('mock');
    expect(resolveRuntimeKind('Tauri')).toBe('tauri');
    expect(resolveRuntimeKind('HTTP')).toBe('http');
  });

  it('rejects an unknown runtime rather than silently defaulting', () => {
    expect(() => resolveRuntimeKind('grpc')).toThrow(RuntimeConfigurationError);
  });

  it('names the offending value and the valid options in the error', () => {
    let caught: unknown;
    try {
      resolveRuntimeKind('grpc');
    } catch (error) {
      caught = error;
    }

    expect(caught).toBeInstanceOf(RuntimeConfigurationError);
    const message = (caught as RuntimeConfigurationError).message;
    expect(message).toContain('grpc');
    expect(message).toContain('VITE_RUNTIME');
    for (const kind of RUNTIME_KINDS) {
      expect(message).toContain(kind);
    }
  });
});
