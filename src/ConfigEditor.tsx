import Editor, { loader } from '@monaco-editor/react';
import * as monaco from 'monaco-editor/editor/editor.api.js';
import EditorWorker from 'monaco-editor/editor/editor.worker.js?worker';

monaco.languages.register({ id: 'larager-config' });
monaco.languages.setMonarchTokensProvider('larager-config', {
  tokenizer: {
    root: [
      [/^\s*[;#].*$/, 'comment'],
      [/\[[^\]]+\]/, 'type'],
      [/\b(?:on|off|true|false|yes|no)\b/i, 'keyword'],
      [/"[^"\n]*"|'[^'\n]*'/, 'string'],
      [/\b\d+\b/, 'number'],
      [/[{}=]/, 'delimiter'],
    ],
  },
});

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

export default function ConfigEditor({
  value,
  onChange,
  readOnly,
  theme,
}: {
  value: string;
  onChange: (value: string) => void;
  readOnly: boolean;
  theme: string;
}) {
  return (
    <Editor
      height="420px"
      language="larager-config"
      value={value}
      onChange={(value) => onChange(value ?? '')}
      theme={theme === 'dark' ? 'vs-dark' : 'light'}
      options={{
        readOnly,
        minimap: { enabled: false },
        fontFamily: 'Cascadia Code, Consolas, monospace',
        fontSize: 13,
        lineHeight: 22,
        padding: { top: 16 },
        scrollBeyondLastLine: false,
        wordWrap: 'on',
        automaticLayout: true,
        renderLineHighlight: 'none',
        overviewRulerLanes: 0,
        accessibilitySupport: 'on',
        fixedOverflowWidgets: true,
        tabSize: 4,
      }}
    />
  );
}
