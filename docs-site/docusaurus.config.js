const { themes } = require('prism-react-renderer');

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'AYX-RS Docs',
  tagline: 'Versioned docs for the ayx CLI and its release surface.',
  favicon: 'img/logo.svg',
  url: 'https://ayx-rs.pages.dev',
  baseUrl: '/',
  organizationName: 'RyanMerlin',
  projectName: 'ayx-rs',
  onBrokenLinks: 'throw',
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },
  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
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
            { label: 'Command surface', to: '/reference/command-surface' },
            { label: 'Release notes', to: '/releases' },
          ],
        },
        {
          title: 'Source',
          items: [
            { label: 'GitHub repo', href: 'https://github.com/RyanMerlin/ayx-rs' },
            { label: 'README', href: 'https://github.com/RyanMerlin/ayx-rs/blob/main/README.md' },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} AYX Team`,
    },
    prism: {
      theme: themes.github,
      darkTheme: themes.dracula,
    },
    colorMode: {
      defaultMode: 'light',
      disableSwitch: false,
      respectPrefersColorScheme: true,
    },
    metadata: [
      { name: 'description', content: 'Versioned documentation for ayx-rs, the Alteryx operator CLI.' },
    ],
  },
};

module.exports = config;
