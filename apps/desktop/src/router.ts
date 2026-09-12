import { createRouter, createWebHashHistory } from 'vue-router'
import OverviewView from './views/OverviewView.vue'
import PortsView from './views/PortsView.vue'
import ProcessesView from './views/ProcessesView.vue'
import ProjectView from './views/ProjectView.vue'
import ProjectsView from './views/ProjectsView.vue'
import ServicesView from './views/ServicesView.vue'
import SettingsView from './views/SettingsView.vue'
import UtilitiesView from './views/UtilitiesView.vue'

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
    { path: '/settings', name: 'settings', component: SettingsView },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})
