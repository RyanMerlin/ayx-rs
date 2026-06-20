const { themes } = require('prism-react-renderer');

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'AYX-RS Docs',
  tagline: 'CLI toolset for Alteryx administrators — automation, agentic workflows, and structured output.',
  favicon: 'img/logo.svg',
  url: 'https://ayx-rs.pages.dev',
  baseUrl: '/',
  organizationName: 'RyanMerlin',
  projectName: 'ayx-rs',
  onBrokenLinks: 'throw',
  markdown: {
    // Parse .md as CommonMark (lenient) so generated CLI docs with raw <br> in
    // table cells don't trip MDX's JSX parser. .mdx would still be MDX (none today).
    format: 'detect',
  },
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },
  presets: [
    [
      'classic',
      {
        docs: {
          path: 'docs',
          routeBasePath: '/',
          sidebarPath: require.resolve('./sidebars.js'),
          showLastUpdateAuthor: false,
          showLastUpdateTime: false,
        },
        blog: false,
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      },
    ],
    [
      'redocusaurus',
      {
        specs: [
          {
            id: 'alteryx-server-api-v3',
            spec: 'static/swagger-v3.json',
            route: '/reference/api/',
          },
        ],
        theme: {
          primaryColor: '#0066cc',
        },
      },
    ],
  ],
  themeConfig: {
    navbar: {
      title: 'AYX-RS Docs',
      logo: {
        alt: 'AYX-RS logo',
        src: 'img/logo.svg',
      },
      items: [
        { to: '/', label: 'Start', position: 'left' },
        { to: '/reference/command-surface', label: 'Commands', position: 'left' },
        { to: '/reference/api/', label: 'API', position: 'left' },
        { to: '/releases', label: 'Releases', position: 'left' },
        { href: 'https://github.com/RyanMerlin/ayx-rs', label: 'GitHub', position: 'right' },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            { label: 'Getting started', to: '/getting-started' },
            { label: 'Safety model', to: '/safety-model' },
            { label: 'Command surface', to: '/reference/command-surface' },
            { label: 'Release notes', to: '/releases' },
          ],
        },
        {
          title: 'Reference',
          items: [
            { label: 'CLI spec', to: '/reference/cli-spec' },
            { label: 'Runtime config', to: '/reference/runtime-config-contract' },
            { label: 'API Reference', to: '/reference/api/' },
          ],
        },
        {
          title: 'Source',
          items: [
            { label: 'GitHub repo', href: 'https://github.com/RyanMerlin/ayx-rs' },
            { label: 'Releases', href: 'https://github.com/RyanMerlin/ayx-rs/releases' },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} AYX Team`,
    },
    prism: {
      theme: themes.github,
      darkTheme: themes.dracula,
      additionalLanguages: ['bash', 'powershell', 'yaml'],
    },
    colorMode: {
      defaultMode: 'light',
      disableSwitch: false,
      respectPrefersColorScheme: true,
    },
    metadata: [
      { name: 'description', content: 'Documentation for ayx-rs, the Alteryx operator CLI — command reference, configuration, and release notes.' },
    ],
  },
};

module.exports = config;
