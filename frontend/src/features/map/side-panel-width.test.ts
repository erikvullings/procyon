import { describe, expect, it } from 'vitest';
import {
  clampFraction,
  MAX_SIDE_FRACTION,
  MIN_SIDE_FRACTION,
  setSidePanelFraction,
  sidePanelFraction,
  sidePanelStyle,
} from './side-panel-width';

describe('side panel width', () => {
  it('clamps to the allowed range', () => {
    expect(clampFraction(0)).toBe(MIN_SIDE_FRACTION);
    expect(clampFraction(5)).toBe(MAX_SIDE_FRACTION);
    expect(clampFraction(0.4)).toBe(0.4);
  });

  it('exposes the shared fraction as a percentage flex-basis', () => {
    setSidePanelFraction(0.25);
    expect(sidePanelFraction()).toBe(0.25);
    expect(sidePanelStyle()).toEqual({ flexBasis: '25.00%' });
  });
});
