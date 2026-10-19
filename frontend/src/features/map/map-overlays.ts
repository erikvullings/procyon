/** An image draped over a rectangle, optionally rotated, ready for a MapLibre image source. */
export interface MapOverlay {
  readonly id: string;
  /** `data:` URI of the image. */
  readonly url: string;
  readonly north: number;
  readonly south: number;
  readonly east: number;
  readonly west: number;
  /** Counter-clockwise degrees around the centre (KML convention). */
  readonly rotation?: number;
}

type Corner = [number, number];

/**
 * The four corners (top-left, top-right, bottom-right, bottom-left) in longitude/latitude. A
 * rotation turns the rectangle around its centre; longitude offsets are scaled by cos(latitude)
 * so that the shape stays rectangular on the ground.
 */
export function overlayCorners(overlay: MapOverlay): [Corner, Corner, Corner, Corner] {
  const { north, south, east, west } = overlay;
  const corners: [Corner, Corner, Corner, Corner] = [
    [west, north],
    [east, north],
    [east, south],
    [west, south],
  ];
  const degrees = overlay.rotation ?? 0;
  if (degrees === 0) return corners;
  const centreLongitude = (east + west) / 2;
  const centreLatitude = (north + south) / 2;
  const scale = Math.cos((centreLatitude * Math.PI) / 180) || 1;
  const radians = (degrees * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  return corners.map(([longitude, latitude]): Corner => {
    const x = (longitude - centreLongitude) * scale;
    const y = latitude - centreLatitude;
    return [centreLongitude + (x * cos - y * sin) / scale, centreLatitude + x * sin + y * cos];
  }) as [Corner, Corner, Corner, Corner];
}
