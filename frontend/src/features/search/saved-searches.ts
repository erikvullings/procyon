import type { SavedSearch, SearchQuery } from '../../models';

function pinnedFirst(searches: readonly SavedSearch[]): readonly SavedSearch[] {
  return [...searches].sort((left, right) => Number(right.pinned) - Number(left.pinned));
}

export function saveSearch(
  searches: readonly SavedSearch[],
  savedSearch: SavedSearch,
): readonly SavedSearch[] {
  if (searches.some(({ id }) => id === savedSearch.id)) {
    return updateSavedSearch(searches, savedSearch.id, savedSearch.query).map((candidate) =>
      candidate.id === savedSearch.id
        ? { ...candidate, name: savedSearch.name, pinned: savedSearch.pinned }
        : candidate,
    );
  }
  return pinnedFirst([...searches, savedSearch]);
}

export function renameSavedSearch(
  searches: readonly SavedSearch[],
  id: string,
  name: string,
): readonly SavedSearch[] {
  const trimmed = name.trim();
  if (trimmed.length === 0) return searches;
  return searches.map((saved) => (saved.id === id ? { ...saved, name: trimmed } : saved));
}

export function updateSavedSearch(
  searches: readonly SavedSearch[],
  id: string,
  query: SearchQuery,
): readonly SavedSearch[] {
  return searches.map((saved) => (saved.id === id ? { ...saved, query } : saved));
}

export function toggleSavedSearchPin(
  searches: readonly SavedSearch[],
  id: string,
): readonly SavedSearch[] {
  return pinnedFirst(
    searches.map((saved) => (saved.id === id ? { ...saved, pinned: !saved.pinned } : saved)),
  );
}

export function deleteSavedSearch(
  searches: readonly SavedSearch[],
  id: string,
): readonly SavedSearch[] {
  return searches.filter((saved) => saved.id !== id);
}
