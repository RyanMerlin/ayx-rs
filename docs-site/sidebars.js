/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docsSidebar: [
    'intro',
    'getting-started',
    'configuration',
    'safety-model',
    {
      type: 'category',
      label: 'Reference',
      collapsed: false,
      items: [
        'reference/command-surface',
        'reference/cli-spec',
        'reference/runtime-config-contract',
        {
          type: 'link',
          label: 'API Reference (Alteryx Server V3)',
          href: '/reference/api/',
        },
      ],
    },
    {
      type: 'category',
      label: 'Releases',
      items: ['releases/index', 'releases/v0.9.10', 'releases/v0.9.9'],
    },
    'troubleshooting',
    'contributing',
  ],
};

module.exports = sidebars;
