// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLinksValidator from 'starlight-links-validator';
import { sidebar } from './sidebar.mjs';

const GITHUB = 'https://github.com/softberries/ramzes';

// Code block surfaces follow the site tokens (src/styles/theme.css): --rz-code-bg per theme.
/** @param {import('@astrojs/starlight/expressive-code').ExpressiveCodeTheme} theme */
function tintCodeTheme(theme) {
	const dark = theme.type === 'dark';
	theme.colors['editor.background'] = dark ? '#0b1022' : '#f7f8fb';
	theme.colors['editorGroupHeader.tabsBackground'] = dark ? '#0f152a' : '#eceff7';
	theme.colors['tab.activeBackground'] = dark ? '#0b1022' : '#f7f8fb';
	theme.colors['terminal.background'] = dark ? '#0b1022' : '#f7f8fb';
	return theme;
}

export default defineConfig({
	site: 'https://softberries.github.io',
	base: '/ramzes/',
	trailingSlash: 'always',
	outDir: './dist',
	// Inline the CSS so it does not block the first paint with extra requests.
	build: { inlineStylesheets: 'always' },
	integrations: [
		starlight({
			title: 'ramzes',
			description:
				'ramzes is a self-hosted notification router: services send events to one place, and routes decide who is told, where, and how often.',
			logo: { src: './src/assets/logo.svg', alt: '' },
			favicon: '/favicon.svg',
			head: [{ tag: 'meta', attrs: { name: 'theme-color', content: '#090d1a' } }],
			social: [
				{ icon: 'github', label: 'GitHub', href: GITHUB },
				{ icon: 'email', label: 'Email Softberries', href: 'mailto:hello@softberries.dev' },
			],
			editLink: { baseUrl: `${GITHUB}/edit/main/site/` },
			lastUpdated: false,
			customCss: [
				'@fontsource-variable/ibm-plex-sans/wght.css',
				'@fontsource/ibm-plex-mono/400.css',
				'@fontsource/ibm-plex-mono/500.css',
				'./src/styles/theme.css',
			],
			components: {
				Hero: './src/components/Hero.astro',
			},
			expressiveCode: {
				themes: ['github-dark-default', 'github-light-default'],
				useStarlightUiThemeColors: true,
				customizeTheme: tintCodeTheme,
				styleOverrides: {
					borderRadius: '10px',
					borderColor: 'var(--sl-color-hairline-light)',
					codeFontFamily: 'var(--__sl-font-mono)',
					uiFontFamily: 'var(--__sl-font)',
					codeFontSize: '0.8125rem',
					codeLineHeight: '1.65',
					codePaddingBlock: '1rem',
					codePaddingInline: '1.15rem',
					frames: {
						shadowColor: 'transparent',
						editorActiveTabIndicatorTopColor: 'var(--rz-gold)',
						frameBoxShadowCssValue: 'var(--rz-shadow-code)',
					},
				},
			},
			plugins: [starlightLinksValidator({ errorOnRelativeLinks: false })],
			sidebar,
		}),
	],
});
