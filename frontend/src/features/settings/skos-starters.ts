import sustainableFood from './skos-starters/agrovoc-sustainable-food.json';
import climateEnergy from './skos-starters/gemet-climate-energy.json';
import defence from './skos-starters/unesco-defence.json';
import technology from './skos-starters/unesco-information-technology.json';
import science from './skos-starters/unesco-natural-sciences.json';
import psychology from './skos-starters/unesco-psychology.json';

const sourceKey = 'https://procyon.app/ns/skos-starter/source';
const licenseKey = 'https://procyon.app/ns/skos-starter/license';

export const skosStarters = [
  {
    document: sustainableFood,
    publisher: 'Food and Agriculture Organization of the United Nations (FAO)',
    version: '2026-10-07 snapshot',
    sourceUrl: sustainableFood.extensions[sourceKey],
    licenseUrl: sustainableFood.extensions[licenseKey],
    license: 'CC BY 4.0',
  },
  {
    document: climateEnergy,
    publisher: 'European Environment Agency (EEA)',
    version: 'GEMET 4.2.3',
    sourceUrl: climateEnergy.extensions[sourceKey],
    licenseUrl: climateEnergy.extensions[licenseKey],
    license: 'CC BY 4.0',
  },
  {
    document: science,
    publisher: 'UNESCO',
    version: '2026-08-31 snapshot',
    sourceUrl: science.extensions[sourceKey],
    licenseUrl: science.extensions[licenseKey],
    license: 'CC BY-SA 3.0 IGO',
  },
  {
    document: technology,
    publisher: 'UNESCO',
    version: '2026-08-31 snapshot',
    sourceUrl: technology.extensions[sourceKey],
    licenseUrl: technology.extensions[licenseKey],
    license: 'CC BY-SA 3.0 IGO',
  },
  {
    document: psychology,
    publisher: 'UNESCO',
    version: '2026-08-31 snapshot',
    sourceUrl: psychology.extensions[sourceKey],
    licenseUrl: psychology.extensions[licenseKey],
    license: 'CC BY-SA 3.0 IGO',
  },
  {
    document: defence,
    publisher: 'UNESCO',
    version: '2026-08-31 snapshot',
    sourceUrl: defence.extensions[sourceKey],
    licenseUrl: defence.extensions[licenseKey],
    license: 'CC BY-SA 3.0 IGO',
  },
] as const;
