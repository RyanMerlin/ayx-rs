import React from 'react';
import ComponentCreator from '@docusaurus/ComponentCreator';

export default [
  {
    path: '/reference/api/',
    component: ComponentCreator('/reference/api/', '608'),
    exact: true
  },
  {
    path: '/',
    component: ComponentCreator('/', '2e1'),
    exact: true
  },
  {
    path: '/',
    component: ComponentCreator('/', '795'),
    routes: [
      {
        path: '/',
        component: ComponentCreator('/', 'fe7'),
        routes: [
          {
            path: '/',
            component: ComponentCreator('/', '845'),
            routes: [
              {
                path: '/configuration',
                component: ComponentCreator('/configuration', '0ad'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/contributing',
                component: ComponentCreator('/contributing', '6d1'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/getting-started',
                component: ComponentCreator('/getting-started', '5d2'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/intro',
                component: ComponentCreator('/intro', '4a2'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/reference/cli-spec',
                component: ComponentCreator('/reference/cli-spec', 'cc6'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/reference/command-surface',
                component: ComponentCreator('/reference/command-surface', 'f97'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/reference/runtime-config-contract',
                component: ComponentCreator('/reference/runtime-config-contract', 'b88'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/releases/',
                component: ComponentCreator('/releases/', '09e'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/releases/v0.9.10',
                component: ComponentCreator('/releases/v0.9.10', '654'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/releases/v0.9.9',
                component: ComponentCreator('/releases/v0.9.9', '60d'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/safety-model',
                component: ComponentCreator('/safety-model', 'a47'),
                exact: true,
                sidebar: "docsSidebar"
              },
              {
                path: '/troubleshooting',
                component: ComponentCreator('/troubleshooting', 'c3d'),
                exact: true,
                sidebar: "docsSidebar"
              }
            ]
          }
        ]
      }
    ]
  },
  {
    path: '*',
    component: ComponentCreator('*'),
  },
];
