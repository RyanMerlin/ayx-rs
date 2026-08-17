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
          items: [
            'getting-started',
            'connecting',
            'configuration',
            'safety-model',
            'output-automation',
            'common-tasks',
            'troubleshooting',
          ],
        },
        {
          label: 'Alteryx One',
          collapsed: false,
          items: [
            'one',
            {
              label: 'Workflows',
              collapsed: false,
              items: [
                'one/workflows',
                'one/workflows/inspect',
                'one/workflows/share',
                'one/workflows/delete',
              ],
            },
            {
              label: 'Connections',
              collapsed: false,
              items: [
                'one/connections',
                'one/connections/connector-metadata',
                'one/connections/permissions',
              ],
            },
            'one/datasets',
            {
              label: 'Jobs & runs',
              collapsed: true,
              items: [
                'one/job-groups',
                'one/job-groups/results',
                'one/output-objects',
              ],
            },
            {
              label: 'Scheduling',
              collapsed: true,
              items: [
                'one/scheduling',
                'one/plans',
                'one/plans/schedules',
                'one/plans/import-export',
              ],
            },
            {
              label: 'Identity & users',
              collapsed: true,
              items: [
                'one/identity',
                'one/workspace',
                'one/person',
                'one/role',
                'one/token',
              ],
            },
            {
              label: 'Settings & admin',
              collapsed: true,
              items: [
                'one/write-settings',
                'one/webhooks',
                {
                  label: 'Diagnostics',
                  collapsed: true,
                  items: ['one/diagnostics', 'one/diagnostics/api'],
                },
              ],
            },
            {
              label: 'Flows (DC Legacy)',
              collapsed: true,
              items: [
                'one/flows',
                'one/flows/folders',
                'one/flows/import-export',
                'one/flows/permissions',
              ],
            },
          ],
        },
        {
          label: 'Alteryx Server',
          collapsed: true,
          items: [
            'server',
            {
              label: 'Logs & diagnostics',
              collapsed: true,
              items: ['server/logs', 'server/diagnose'],
            },
            'server/upgrade',
            'server/mongo',
            'server/sqlserver',
            'server/workflow',
          ],
        },
        {
          label: 'Telemetry & actions',
          collapsed: true,
          items: ['telemetry', 'telemetry/actions'],
        },
        {
          label: 'Reference',
          collapsed: true,
          items: [{ autogenerate: { directory: 'reference' } }],
        },
        {
          label: 'Releases',
          collapsed: true,
          items: [{ autogenerate: { directory: 'releases' } }],
        },
      ],
      pagefind: true,
    }),
  ],
});
