import { PrismLight as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark, oneLight } from 'react-syntax-highlighter/dist/esm/styles/prism';
import rust from 'react-syntax-highlighter/dist/esm/languages/prism/rust';
import python from 'react-syntax-highlighter/dist/esm/languages/prism/python';
import javascript from 'react-syntax-highlighter/dist/esm/languages/prism/javascript';
import typescript from 'react-syntax-highlighter/dist/esm/languages/prism/typescript';
import tsx from 'react-syntax-highlighter/dist/esm/languages/prism/tsx';
import go from 'react-syntax-highlighter/dist/esm/languages/prism/go';

// Register only the languages we index — keeps the bundle small.
SyntaxHighlighter.registerLanguage('rust', rust);
SyntaxHighlighter.registerLanguage('python', python);
SyntaxHighlighter.registerLanguage('javascript', javascript);
SyntaxHighlighter.registerLanguage('typescript', typescript);
SyntaxHighlighter.registerLanguage('tsx', tsx);
SyntaxHighlighter.registerLanguage('go', go);

interface Props {
  code: string;
  lang: string;
  startLine?: number;
  isDark: boolean;
}

/** Syntax-highlighted source view with line numbers. */
export function CodeBlock({ code, lang, startLine = 1, isDark }: Props) {
  return (
    <SyntaxHighlighter
      language={lang}
      style={isDark ? oneDark : oneLight}
      showLineNumbers
      startingLineNumber={startLine}
      wrapLongLines
      customStyle={{
        margin: 0,
        borderRadius: 8,
        fontSize: 12.5,
        background: 'transparent',
        maxHeight: 420,
      }}
      lineNumberStyle={{ opacity: 0.4, minWidth: 36 }}
      codeTagProps={{ style: { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' } }}
    >
      {code}
    </SyntaxHighlighter>
  );
}
