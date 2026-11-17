tailwind.config = {
  theme: {
    extend: {
      fontFamily: {
        sans: ['Inter', '-apple-system', 'BlinkMacSystemFont', 'Segoe UI', 'Roboto', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
      },
      colors: {
        spot: {
          void: '#0b0c10',
          black: '#12141a',
          elevated: '#171922',
          highlight: '#202430',
          muted: '#9aa7bc',
          faint: '#7b89a0',
          green: '#22c55e',
          bright: '#4ade80',
          dim: '#16a34a',
        }
      }
    }
  }
};
