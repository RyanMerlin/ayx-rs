import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://ayx-rs.pages.dev',
  base: '/',
  integrations: [
    starlight({
      title: 'ayx-rs',
      tagline: 'A precision operator CLI for Alteryx administrators.',
      description:
        'Documentation for ayx — a command-line operator for Alteryx One and Alteryx Server: generated command reference, configuration contracts, the safety model, and versioned release notes.',
      customCss: ['./src/styles/custom.css'],
      components: {
        SiteTitle: './src/components/SiteTitle.astro',
      },
      favicon: '/favicon.svg',
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/RyanMerlin/ayx-rs' },
      ],
      sidebar: [
        {
          label: 'Guides',
          items: ['getting-started', 'configuration', 'safety-model', 'troubleshooting'],
        },
        { label: 'Reference', autogenerate: { directory: 'reference' } },
        { label: 'Releases', autogenerate: { directory: 'releases' } },
      ],
      pagefind: true,
    }),
  ],
});
