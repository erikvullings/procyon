import { describe, expect, it } from 'vitest';
import { generationErrorText } from './generation-error';

describe('generationErrorText', () => {
  it('shows the localised reason of a rejection', () => {
    expect(
      generationErrorText(
        { code: 'invalidRequest', message: 'x', details: { reason: 'llmModelUnavailable' } },
        'generic',
      ),
    ).toContain('AI model is not available');
    expect(
      generationErrorText(
        { code: 'platformOperationFailed', message: 'x', details: { reason: 'llmTransport' } },
        'generic',
      ),
    ).toBe('The AI service could not be reached.');
  });

  it('keeps the generic text for anything else', () => {
    expect(generationErrorText({ code: 'internal', message: 'boom' }, 'generic')).toBe('generic');
    expect(generationErrorText(new Error('boom'), 'generic')).toBe('generic');
    expect(generationErrorText(undefined, 'generic')).toBe('generic');
  });
});
