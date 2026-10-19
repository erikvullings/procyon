import type { FileViewerAttrs } from './file-viewer';
import type { FileViewerController, FileViewerState } from './file-viewer-controller';

/** Host-specific parts of {@link FileViewerAttrs}: everything the controller cannot provide. */
export type FileViewerAttrsExtras = Pick<
  FileViewerAttrs,
  'quickLookAvailable' | 'onQuickLook' | 'onOpenExternally' | 'onClose'
> &
  Partial<
    Pick<
      FileViewerAttrs,
      | 'videoPosterDataUri'
      | 'onDocumentSummary'
      | 'onPlayTracks'
      | 'onOpenOnMap'
      | 'mapClient'
      | 'links'
    >
  > & {
    readonly openExternalUrl: (url: string) => void;
  };

/** Wires a viewer controller to the presentational {@link FileViewer} component, shared by the
 * in-pane viewer tab and the separate viewer window so both behave identically. */
export function fileViewerAttrs(
  state: FileViewerState,
  controller: FileViewerController,
  { openExternalUrl, ...extras }: FileViewerAttrsExtras,
): FileViewerAttrs {
  return {
    state,
    onLoadMore: () => void controller.loadMore(),
    onLoadPrevious: () => void controller.loadPrevious(),
    onLoadTextPage: (pageIndex) => void controller.loadTextPage(pageIndex),
    onLoadStructuredRows: (startRow) => void controller.loadStructuredRows(startRow),
    onStructuredOptionsChange: (delimiter, headerMode) =>
      void controller.setStructuredOptions(delimiter, headerMode),
    onSelectStructuredSheet: (sheetName) => void controller.selectStructuredSheet(sheetName),
    onToggleStructuredRowNumbers: () => controller.toggleStructuredRowNumbers(),
    onLoadJsonWindow: (offset) => void controller.loadJsonWindow(offset),
    onSearchStructuredRows: (query, cursor) => void controller.searchStructuredRows(query, cursor),
    onSortStructuredRows: (column) => void controller.sortStructuredRows(column),
    sqlite: {
      onTab: (tab) => controller.setSqliteTab(tab),
      onSelectObject: (name) => controller.selectSqliteObject(name),
      onBrowse: (patch) => controller.updateSqliteBrowse(patch),
      onPage: (slot, offset) => controller.loadSqlitePage(slot, offset),
      onSql: (sql) => controller.setSqliteSql(sql),
      onRunSql: () => controller.runSqliteSql(),
    },
    onSearchQueryChange: (query) => controller.setSearchOptions({ query }),
    onSearchOptionChange: (patch) => controller.setSearchOptions(patch),
    onRunSearch: () => void controller.runSearch(),
    onNextMatch: () => void controller.goToNextMatch(),
    onPreviousMatch: () => void controller.goToPreviousMatch(),
    onZoomIn: () => controller.zoomIn(),
    onZoomOut: () => controller.zoomOut(),
    onZoomChange: (zoom) => controller.setZoom(zoom),
    onResetZoom: () => controller.resetZoom(),
    onCopy: () => controller.copyContent(),
    onToggleMetadata: () => controller.toggleMetadataPanel(),
    onSelectGeoFeature: (index) => controller.selectGeoFeature(index),
    onToggleGeoRaw: () => controller.toggleGeoRaw(),
    onNextPage: () => controller.nextPage(),
    onPreviousPage: () => controller.previousPage(),
    onPdfSearchQueryChange: (query) => controller.setPdfSearchQuery(query),
    onNextPdfMatch: () => controller.goToNextPdfMatch(),
    onPreviousPdfMatch: () => controller.goToPreviousPdfMatch(),
    onEpubSearchQueryChange: (query) => controller.setEpubSearchQuery(query),
    onNextEpubMatch: () => controller.goToNextEpubMatch(),
    onPreviousEpubMatch: () => controller.goToPreviousEpubMatch(),
    onSelectEpubSection: (sectionIndex, fragment) =>
      controller.goToEpubSection(sectionIndex, fragment),
    onFollowEpubLink: (href) => controller.followEpubLink(href),
    onSelectPdfPage: (pageNumber) => controller.goToPdfPage(pageNumber),
    onNavigateTextOffset: (offset, length) => controller.goToTextOffset(offset, length),
    onOpenExternalLink: openExternalUrl,
    ...extras,
  };
}
