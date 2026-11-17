import { useEffect, useState } from 'react';

export type ThemeMode = 'system' | 'light' | 'dark';

export function useTheme() {
  const [theme, setTheme] = useState<ThemeMode>(() => {
    try {
      return (localStorage.getItem('refract.theme') as ThemeMode) || 'system';
    } catch {
      return 'system';
    }
  });

  useEffect(() => {
    try {
      localStorage.setItem('refract.theme', theme);
    } catch {}

    const apply = () => {
      const isDark =
        theme === 'dark' ||
        (theme === 'system' && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
      if (isDark) {
        document.documentElement.setAttribute('data-theme', 'dark');
        document.documentElement.classList.add('dark');
      } else {
        document.documentElement.setAttribute('data-theme', 'light');
        document.documentElement.classList.remove('dark');
      }
    };

    apply();

    if (theme === 'system') {
      const media = window.matchMedia('(prefers-color-scheme: dark)');
      media.addEventListener('change', apply);
      return () => media.removeEventListener('change', apply);
    }
  }, [theme]);

  const toggleTheme = () => {
    setTheme((curr) => (curr === 'system' ? 'light' : curr === 'light' ? 'dark' : 'system'));
  };

  return { theme, setTheme, toggleTheme };
}
