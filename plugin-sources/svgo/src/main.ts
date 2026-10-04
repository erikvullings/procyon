import m from 'mithril';
import './styles.css';
import { App, initializeGlobalHandlers } from './app';

const root = document.getElementById('app');
if (root) {
  m.mount(root, App);
  initializeGlobalHandlers();
}
