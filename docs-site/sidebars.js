/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docsSidebar: [
    'intro',
    'getting-started',
    'configuration',
    'reference/command-surface',
    'reference/cli-spec',
    'reference/runtime-config-contract',
    {
      type: 'category',
      label: 'Releases',
      items: ['releases/index', 'releases/v0.9.10', 'releases/v0.9.9'],
    },
    'operations/public-release-checklist',
    'troubleshooting',
    'contributing',
  ],
};

module.exports = sidebars;
