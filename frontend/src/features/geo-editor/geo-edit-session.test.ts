import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GeoDocument } from '../../models';
import { createGeoEditSession, MAX_EDIT_BYTES } from './geo-edit-session';

const SAMPLE = JSON.stringify({
  type: 'FeatureCollection',
  features: [
    { type: 'Feature', geometry: { type: 'Point', coordinates: [4, 52] }, properties: { a: 1 } },
  ],
});

function parseResult(error?: GeoDocument['error']): GeoDocument {
  return {
    format: 'geojson',
    collection: { type: 'FeatureCollection', features: [] },
    totalFeatures: 0,
    displayedFeatures: 0,
    truncated: false,
    warnings: [],
    skipped: [],
    ...(error === undefined ? {} : { error }),
  };
}

function setup(initial: string = SAMPLE, validator?: (text: string) => GeoDocument) {
  let content = initial;
  const setContent = vi.fn((text: string) => {
    content = text;
  });
  const changed = vi.fn();
  const client = {
    parseGeoText: vi.fn(async (text: string) => (validator ?? (() => parseResult()))(text)),
  };
  const session = createGeoEditSession({
    getContent: () => content,
    setContent,
    client,
    changed,
  });
  return {
    session,
    setContent,
    changed,
    client,
    get content() {
      return content;
    },
    set content(text: string) {
      content = text;
    },
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('GeoEditSession', () => {
  it('loads the document and starts in select mode', () => {
    const { session } = setup();
    expect(session.state()).toMatchObject({
      status: 'ready',
      tool: 'select',
      canUndo: false,
      wrapped: false,
    });
    expect(session.state().document?.features).toHaveLength(1);
  });

  it('writes every edit back to the editor text and supports undo and redo', () => {
    const harness = setup();
    const { session } = harness;
    const id = session.drawn({ type: 'Point', coordinates: [1, 2] });
    expect(JSON.parse(harness.content).features).toHaveLength(2);
    expect(session.state().selectedId).toBe(id);
    expect(session.state().canUndo).toBe(true);
    session.undo();
    expect(JSON.parse(harness.content).features).toHaveLength(1);
    expect(session.state().selectedId).toBeUndefined();
    session.redo();
    expect(JSON.parse(harness.content).features).toHaveLength(2);
  });

  it('moves, duplicates and removes features', () => {
    const harness = setup();
    const { session } = harness;
    const id = session.state().document?.ids[0] as string;
    session.geometryChanged(id, { type: 'Point', coordinates: [9, 9] });
    expect(JSON.parse(harness.content).features[0].geometry.coordinates).toEqual([9, 9]);
    session.duplicate(id);
    expect(session.state().document?.features).toHaveLength(2);
    expect(session.state().selectedId).toBe(session.state().document?.ids[1]);
    session.remove(id);
    expect(session.state().document?.features).toHaveLength(1);
  });

  it('rewrites the text compact or indented without touching the data or the undo history', () => {
    const harness = setup();
    harness.session.reformat(true);
    expect(harness.content.trim().split('\n')).toHaveLength(1);
    expect(harness.session.state().canUndo).toBe(false);
    harness.session.reformat(false);
    expect(harness.content.split('\n').length).toBeGreaterThan(5);
    expect(JSON.parse(harness.content)).toEqual(JSON.parse(SAMPLE));
    expect(harness.session.state().rawText).toBe(harness.content);
  });

  it('nudges a feature by a few metres', () => {
    const harness = setup();
    const id = harness.session.state().document?.ids[0] as string;
    harness.session.nudge(id, 0.00001, -0.00001);
    const [lon, lat] = JSON.parse(harness.content).features[0].geometry.coordinates;
    expect(lon).toBeCloseTo(4.00001, 7);
    expect(lat).toBeCloseTo(51.99999, 7);
  });

  it('edits typed properties and refuses values of the wrong type', () => {
    const harness = setup();
    const { session } = harness;
    const id = session.state().document?.ids[0] as string;
    expect(session.setProperty(id, 'a', '42', 'number')).toBeUndefined();
    expect(JSON.parse(harness.content).features[0].properties.a).toBe(42);
    expect(session.setProperty(id, 'a', 'x', 'number')).toBe('Enter a number.');
    expect(JSON.parse(harness.content).features[0].properties.a).toBe(42);
    session.addProperty(id, 'note');
    session.renameProperty(id, 'note', 'remark');
    expect(JSON.parse(harness.content).features[0].properties).toEqual({ a: 42, remark: '' });
    session.deleteProperty(id, 'remark');
    expect(JSON.parse(harness.content).features[0].properties).toEqual({ a: 42 });
  });

  it('applies valid raw JSON after a pause and keeps invalid text out of the document', async () => {
    const harness = setup(SAMPLE, (text) =>
      text.includes('BROKEN') ? parseResult({ message: 'bad', line: 1, column: 2 }) : parseResult(),
    );
    const { session } = harness;
    session.setRaw('{"type":"FeatureCollection","features":[]}');
    expect(session.state().document?.features).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(400);
    expect(session.state().document?.features).toHaveLength(0);
    expect(harness.content).toBe('{"type":"FeatureCollection","features":[]}');

    session.setRaw('BROKEN');
    await vi.advanceTimersByTimeAsync(400);
    expect(session.state().rawError).toMatchObject({ message: 'bad', line: 1 });
    expect(session.state().document?.features).toHaveLength(0);
    expect(session.state().rawText).toBe('BROKEN');
  });

  it('imports pasted GeoJSON by appending and reports invalid input', async () => {
    const harness = setup(SAMPLE, (text) =>
      text === 'nope' ? parseResult({ message: 'invalid' }) : parseResult(),
    );
    const { session } = harness;
    expect(await session.importText('{"type":"Point","coordinates":[5,5]}')).toBeUndefined();
    expect(session.state().document?.features).toHaveLength(2);
    expect(await session.importText('nope')).toBe('invalid');
    expect(session.state().document?.features).toHaveLength(2);
  });

  it('follows changes made to the text elsewhere', () => {
    const harness = setup();
    harness.content = '{"type":"FeatureCollection","features":[]}';
    harness.session.sync();
    expect(harness.session.state().document?.features).toHaveLength(0);
    expect(harness.session.state().canUndo).toBe(false);
  });

  it('reports invalid and oversized content instead of editing it', () => {
    expect(setup('{').session.state()).toMatchObject({ status: 'invalid' });
    const big = setup(
      `{"type":"FeatureCollection","features":[],"pad":"${'x'.repeat(MAX_EDIT_BYTES)}"}`,
    );
    expect(big.session.state()).toMatchObject({ status: 'tooLarge' });
    expect(big.session.drawn({ type: 'Point', coordinates: [0, 0] })).toBeUndefined();
  });

  it('wraps a bare geometry into a collection', () => {
    const harness = setup('{"type":"Point","coordinates":[1,2]}');
    expect(harness.session.state().wrapped).toBe(true);
    expect(JSON.parse(harness.content).type).toBe('FeatureCollection');
  });
});
