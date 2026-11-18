import { useEffect, useRef, useState } from 'react';

export type ThemeMode = 'system' | 'light' | 'dark';

function syncThemeColor(isDark: boolean) {
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute('content', isDark ? '#000000' : '#f5f5f7');
}

export function useTheme() {
  const [theme, setTheme] = useState<ThemeMode>(() => {
    try {
      return (localStorage.getItem('refract.theme') as ThemeMode) || 'system';
    } catch {
      return 'system';
    }
  });
  const isFirstRender = useRef(true);

  useEffect(() => {
    try {
      localStorage.setItem('refract.theme', theme);
    } catch {}

    const apply = (suppressTransitions: boolean) => {
      const root = document.documentElement;
      const isDark =
        theme === 'dark' ||
        (theme === 'system' && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);

      if (suppressTransitions) {
        root.classList.add('theme-switching');
      }

      if (isDark) {
        root.setAttribute('data-theme', 'dark');
        root.classList.add('dark');
      } else {
        root.setAttribute('data-theme', 'light');
        root.classList.remove('dark');
      }
      syncThemeColor(isDark);

      if (suppressTransitions) {
        void root.offsetHeight;
        requestAnimationFrame(() => {
          root.classList.remove('theme-switching');
        });
      }
    };

    apply(!isFirstRender.current);
    isFirstRender.current = false;

    if (theme === 'system') {
      const media = window.matchMedia('(prefers-color-scheme: dark)');
      const onMediaChange = () => apply(true);
      media.addEventListener('change', onMediaChange);
      return () => media.removeEventListener('change', onMediaChange);
    }
  }, [theme]);

  const toggleTheme = () => {
    setTheme((curr) => (curr === 'system' ? 'light' : curr === 'light' ? 'dark' : 'system'));
  };

  return { theme, setTheme, toggleTheme };
}
