import { createRouter, createWebHashHistory } from 'vue-router'
import OverviewView from './views/OverviewView.vue'
import PortsView from './views/PortsView.vue'
import ProcessesView from './views/ProcessesView.vue'
import ProjectView from './views/ProjectView.vue'
import ProjectsView from './views/ProjectsView.vue'
import ServicesView from './views/ServicesView.vue'
import SettingsView from './views/SettingsView.vue'
import UtilitiesView from './views/UtilitiesView.vue'
import ApiWorkbenchView from './views/ApiWorkbenchView.vue'
import DatabaseWorkbenchView from './views/DatabaseWorkbenchView.vue'
import VaultView from './views/VaultView.vue'
import FileWorkbenchView from './views/FileWorkbenchView.vue'

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'overview', component: OverviewView },
    { path: '/projects', name: 'projects', component: ProjectsView },
    { path: '/projects/:id', name: 'project', component: ProjectView },
    { path: '/services', name: 'services', component: ServicesView },
    { path: '/processes', name: 'processes', component: ProcessesView },
    { path: '/ports', name: 'ports', component: PortsView },
    { path: '/utilities', name: 'utilities', component: UtilitiesView },
    { path: '/api', name: 'api', component: ApiWorkbenchView },
    { path: '/database', name: 'database', component: DatabaseWorkbenchView },
    // The vault is its own route on purpose: it is not a utility, and it must be
    // reachable by name so that no generic navigation can land on decrypted data.
    { path: '/vault', name: 'vault', component: VaultView },
    { path: '/files', name: 'files', component: FileWorkbenchView },
    { path: '/settings', name: 'settings', component: SettingsView },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})
