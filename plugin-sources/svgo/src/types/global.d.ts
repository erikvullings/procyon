type PathDataSegment = { type: string; values: number[] };

interface SVGPathElement {
  getPathData?: (options?: { normalize?: boolean }) => PathDataSegment[];
}

type MonacoEditorInstance = {
  getValue: () => string;
  setValue: (value: string) => void;
  onDidChangeModelContent: (listener: () => void) => void;
  layout: () => void;
};

type MonacoLanguageConfig = {
  brackets: [string, string][];
  autoClosingPairs: { open: string; close: string }[];
};

interface MonacoApi {
  languages: {
    register: (config: { id: string }) => void;
    setLanguageConfiguration: (languageId: string, config: MonacoLanguageConfig) => void;
  };
  editor: {
    setTheme: (theme: string) => void;
    create: (
      container: HTMLElement,
      options: {
        value: string;
        language: string;
        theme: string;
        automaticLayout: boolean;
        minimap: { enabled: boolean };
        wordWrap: 'on' | 'off' | 'bounded';
        renderLineHighlight: 'none' | 'line' | 'all' | 'gutter';
        lineNumbersMinChars?: number;
        lineDecorationsWidth?: number;
      }
    ) => MonacoEditorInstance;
  };
}

interface RequireLike {
  (dependencies: string[], callback: () => void): void;
  config: (config: { paths: Record<string, string> }) => void;
}

declare const require: RequireLike;
declare const monaco: MonacoApi;
declare function acquireVsCodeApi(): { postMessage: (message: unknown) => void } | undefined;

interface FileSystemFileHandle {
  createWritable: () => Promise<{
    write: (data: BlobPart) => Promise<void>;
    close: () => Promise<void>;
  }>;
}

interface Window {
  procyonPlugin?: {
    loadToken: string;
    theme?: 'light' | 'dark';
    settings?: Partial<import('../optimizer').ProcyonOptimizerSettings>;
    postMessage(message:
      | { type: "save-svg"; svg: string }
      | { type: "settings-change"; settings: import('../optimizer').ProcyonOptimizerSettings; sequence: number; flush?: true }
      | { type: "close-panel" }
    ): void;
  };
  showSaveFilePicker?: (options?: {
    suggestedName?: string;
    types?: { description: string; accept: Record<string, string[]> }[];
  }) => Promise<FileSystemFileHandle>;
}
