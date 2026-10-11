import climateEnergy from './skos-starters/gemet-climate-energy.json';
import defenceEconomics from './skos-starters/stw-defence-economics.json';
import psychology from './skos-starters/stw-psychology.json';
import technology from './skos-starters/stw-technology.json';

const sourceKey = 'https://procyon.app/ns/skos-starter/source';

export const skosStarters = [
  {
    document: climateEnergy,
    publisher: 'European Environment Agency (EEA)',
    version: 'GEMET 4.2.3',
    sourceUrl: climateEnergy.extensions[sourceKey],
  },
  {
    document: technology,
    publisher: 'ZBW - Leibniz Information Centre for Economics',
    version: 'STW 9.20',
    sourceUrl: technology.extensions[sourceKey],
  },
  {
    document: psychology,
    publisher: 'ZBW - Leibniz Information Centre for Economics',
    version: 'STW 9.20',
    sourceUrl: psychology.extensions[sourceKey],
  },
  {
    document: defenceEconomics,
    publisher: 'ZBW - Leibniz Information Centre for Economics',
    version: 'STW 9.20',
    sourceUrl: defenceEconomics.extensions[sourceKey],
  },
] as const;
