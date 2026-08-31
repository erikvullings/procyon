import { describe, expect, it } from 'vitest';

import type { RuntimeKind } from '../../utilities/runtime';
import { createFileManagerClient } from './create-client';
import { HttpFileManagerClient } from './http-file-manager-client';
import { MockFileManagerClient } from './mock-file-manager-client';
import { TauriFileManagerClient } from './tauri-file-manager-client';

describe('createFileManagerClient', () => {
  it('returns an HttpFileManagerClient for "http"', () => {
    expect(createFileManagerClient('http')).toBeInstanceOf(HttpFileManagerClient);
  });

  it('returns a TauriFileManagerClient for "tauri"', () => {
    expect(createFileManagerClient('tauri')).toBeInstanceOf(TauriFileManagerClient);
  });

  it('returns a MockFileManagerClient for "mock"', () => {
    expect(createFileManagerClient('mock')).toBeInstanceOf(MockFileManagerClient);
  });

  it('throws on an unknown runtime rather than silently defaulting', () => {
    expect(() => createFileManagerClient('grpc' as unknown as RuntimeKind)).toThrow();
  });
});
