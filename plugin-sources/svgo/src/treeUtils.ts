export function getElementByPath(doc: Document, path: string): Element | null {
  const parts = path.split('.');
  let current: Element | null = doc.querySelector('svg');
  if (parts[0] !== '0') return null;
  if (parts.length === 1) return current;

  for (let i = 1; i < parts.length; i++) {
    const index = parseInt(parts[i], 10);
    if (!current || !current.children[index]) return null;
    current = current.children[index] as Element;
  }
  return current;
}

export function getElementInSvgByPath(svg: Element, path: string): Element | null {
  const parts = path.split('.');
  let current: Element | Text = svg;
  if (parts[0] !== '0') return null;

  for (let i = 1; i < parts.length; i++) {
    const indexStr = parts[i];
    const arrayMatch = indexStr.match(/^\[-(\d+)\]$/);
    if (arrayMatch) {
      const textIndex = parseInt(arrayMatch[1], 10) - 1;
      const children = Array.from((current as Element).childNodes) as Array<Element | Text>;
      const textNodes = children.filter(n => n.nodeType === 3) as Text[];
      if (textNodes.length > textIndex) {
        current = textNodes[textIndex];
      } else {
        return null;
      }
      continue;
    }

    const index = parseInt(indexStr, 10);
    if (index < 0) {
      const children = Array.from((current as Element).childNodes) as Array<Element | Text>;
      const textNodes = children.filter(n => n.nodeType === 3) as Text[];
      if (textNodes.length >= Math.abs(index)) {
        current = textNodes[Math.abs(index)];
      } else {
        return null;
      }
    } else {
      current = (current as Element).children[index] as Element;
      if (!current) return null;
    }
  }

  return current.nodeType === 1 ? (current as Element) : null;
}
