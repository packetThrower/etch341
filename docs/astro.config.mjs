// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';

// https://astro.build/config
export default defineConfig({
	site: 'https://packetthrower.github.io',
	base: '/etch341/',
	trailingSlash: 'ignore',
	integrations: [
		starlight({
			title: 'etch341',
			description:
				'Cross-platform CLI + GUI flash programmer for the CH341A USB SPI/I²C interface. Read, erase, write, and verify SPI NOR + I²C EEPROM chips.',
			favicon: '/favicon.svg',
			customCss: ['@packetthrower/site-theme/styles/starlight.css'],

			// Site-wide head additions for discoverability:
			//
			// 1. Open Graph / Twitter image. Without this, link-preview
			//    unfurlers (Slack, Discord, Twitter, iMessage, etc.)
			//    render every share as a bare text card.
			// 2. Google Search Console verification slot — drop the
			//    issued meta tag back in here once the etch341
			//    property is claimed at https://search.google.com/search-console.
			head: [
				{
					tag: 'meta',
					attrs: {
						property: 'og:image',
						content: 'https://packetthrower.github.io/etch341/og-image.png',
					},
				},
				{
					tag: 'meta',
					attrs: {
						property: 'og:image:width',
						content: '1200',
					},
				},
				{
					tag: 'meta',
					attrs: {
						property: 'og:image:height',
						content: '630',
					},
				},
				{
					tag: 'meta',
					attrs: {
						name: 'twitter:image',
						content: 'https://packetthrower.github.io/etch341/og-image.png',
					},
				},
			],
			components: {
				// Shared packetThrower header: global bar (Apps menu, search,
				// theme, GitHub) + etch341's app bar. The landing page at
				// /etch341/ is src/pages/index.astro, outside Starlight.
				Header: '@packetthrower/site-theme/components/StarlightHeader.astro',
			},
			editLink: {
				baseUrl: 'https://github.com/packetThrower/etch341/edit/main/docs/',
			},
			sidebar: [
				// Same groups, in the same order, on every packetThrower docs site.
				{
					label: 'Get started',
					items: [
						{ label: 'Install', slug: 'install' },
						{ label: 'Requirements', slug: 'reference/requirements' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'GUI tour', slug: 'usage/gui' },
						{ label: 'CLI reference', slug: 'usage/cli' },
						{ label: 'SPI flash workflow', slug: 'usage/spi' },
						{ label: 'I²C EEPROM workflow', slug: 'usage/i2c' },
						{ label: 'Wiring + voltage', slug: 'usage/wiring' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Chip database', slug: 'reference/chips' },
						{ label: 'Testing', slug: 'reference/testing' },
					],
				},
				{
					label: 'Project',
					items: [
						{ label: 'Changelog', slug: 'changelog' },
						{
							label: 'Issues',
							link: 'https://github.com/packetThrower/etch341/issues',
							attrs: { target: '_blank', rel: 'noopener' },
						},
					],
				},
			],
			lastUpdated: true,
		}),
		// Explicit `@astrojs/sitemap` config — Starlight auto-pulls
		// the integration, but its default emits `<loc>`-only
		// entries. Adding it here lets us pass a `lastmod` so each
		// URL carries a freshness timestamp. Google Search Console
		// uses lastmod for crawl scheduling; without it, every entry
		// looks equally stale and the crawler is less aggressive
		// about re-indexing changed pages.
		//
		// `new Date()` evaluates at build time, so every page in
		// the sitemap gets the deployment timestamp. Per-page
		// per-file mtime would be more accurate but requires a
		// `serialize()` callback walking git history per URL — not
		// worth the build-time cost for a docs site this small.
		sitemap({
			lastmod: new Date(),
		}),
	],
});
