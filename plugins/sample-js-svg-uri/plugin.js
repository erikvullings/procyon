({
  actions() {
    return [{
      id: 'sample.js-svg-uri.copy',
      title: 'Copy SVG URI',
      description: 'Copies the selected SVG file URI.',
      requires_single_selection: true,
    }];
  },
  invoke(actionId) {
    if (actionId !== 'sample.js-svg-uri.copy') throw new Error('Unknown action');
    const [entry] = host.selected_entry_metadata();
    if (!entry || !entry.name.toLowerCase().endsWith('.svg')) {
      throw new Error('Select one SVG file');
    }
    host.clipboard_write(entry.uri);
  },
})
