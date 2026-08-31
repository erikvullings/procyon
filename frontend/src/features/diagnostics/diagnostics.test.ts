/** Tests for diagnostics view component. */

import { describe, expect, it } from 'vitest';
import type { DiagnosticsView } from './diagnostics';
import { diagnosticsFromDto } from './diagnostics';

describe('Diagnostics', () => {
  describe('diagnosticsFromDto', () => {
    it('converts DTO to domain model', () => {
      const dto = {
        frontendVersion: '0.1.0',
        backendVersion: '0.1.0',
        tauriVersion: '2.0.0',
        platform: 'macOS',
        runtimeCapabilities: {
          runtime: 'tauri',
          platform: 'macos',
          nativeMenus: true,
          platformContextMenu: true,
          nativeFileIcons: true,
          nativeThumbnails: true,
          nativeDragOut: true,
          systemTrash: true,
          revealInSystemFileManager: true,
          openTerminal: true,
          clipboard: true,
          plugins: true,
          serverAdministration: false,
          extendedAttributes: true,
          finderTags: true,
        },
        connectionState: {
          connected: true,
          lastEventReceived: '2026-08-10T12:34:56Z',
          uptimeSeconds: 3600,
          eventsReceived: 42,
          statusMessage: 'Connected',
        },
        loadedPlugins: [
          {
            pluginId: 'plugin-1',
            name: 'Test Plugin',
            enabled: true,
            version: '1.0.0',
            errorCount: 0,
          },
        ],
        recentErrors: [
          {
            timestamp: '2026-08-10T12:34:50Z',
            message: 'Sample error',
            code: 'TEST_ERROR',
            context: 'op-123',
          },
        ],
        operationQueueStatus: {
          queuedCount: 1,
          runningCount: 1,
          pausedCount: 0,
          completedCount: 42,
          totalPendingSize: 1048576,
        },
      };

      const result = diagnosticsFromDto(dto);

      expect(result.frontendVersion).toBe('0.1.0');
      expect(result.backendVersion).toBe('0.1.0');
      expect(result.tauriVersion).toBe('2.0.0');
      expect(result.platform).toBe('macOS');
      expect(result.connectionState.connected).toBe(true);
      expect(result.connectionState.eventsReceived).toBe(42);
      expect(result.loadedPlugins).toHaveLength(1);
      expect(result.loadedPlugins[0]?.name).toBe('Test Plugin');
      expect(result.recentErrors).toHaveLength(1);
      expect(result.operationQueueStatus.queuedCount).toBe(1);
    });

    it('handles missing optional fields', () => {
      const dto = {
        frontendVersion: '0.1.0',
        backendVersion: '0.1.0',
        platform: 'Unknown',
      };

      const result = diagnosticsFromDto(dto);

      expect(result.frontendVersion).toBe('0.1.0');
      expect(result.tauriVersion).toBeUndefined();
      expect(result.connectionState.connected).toBe(false);
      expect(result.loadedPlugins).toEqual([]);
      expect(result.recentErrors).toEqual([]);
    });

    it('handles null/undefined arrays', () => {
      const dto = {
        frontendVersion: '0.1.0',
        backendVersion: '0.1.0',
        platform: 'Linux',
        loadedPlugins: null,
        recentErrors: undefined,
      };

      const result = diagnosticsFromDto(dto);

      expect(result.loadedPlugins).toEqual([]);
      expect(result.recentErrors).toEqual([]);
    });
  });

  describe('DiagnosticsView type', () => {
    it('has all required fields', () => {
      const view: DiagnosticsView = {
        frontendVersion: '1.0.0',
        backendVersion: '1.0.0',
        platform: 'macOS',
        runtimeCapabilities: {
          runtime: 'browserServer',
          platform: 'macos',
          nativeMenus: false,
          platformContextMenu: false,
          nativeFileIcons: false,
          nativeThumbnails: false,
          nativeDragOut: false,
          systemTrash: false,
          revealInSystemFileManager: false,
          openTerminal: false,
          clipboard: true,
          plugins: false,
          serverAdministration: false,
          extendedAttributes: false,
          finderTags: false,
        },
        connectionState: {
          connected: true,
          uptimeSeconds: 0,
          eventsReceived: 0,
          statusMessage: 'Connected',
        },
        loadedPlugins: [],
        recentErrors: [],
        operationQueueStatus: {
          queuedCount: 0,
          runningCount: 0,
          pausedCount: 0,
          completedCount: 0,
          totalPendingSize: 0,
        },
      };

      expect(view.frontendVersion).toBeDefined();
      expect(view.runtimeCapabilities).toBeDefined();
      expect(view.connectionState).toBeDefined();
    });
  });
});
