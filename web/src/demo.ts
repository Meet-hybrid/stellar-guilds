/** Guilds deployed by `scripts/testnet-demo.sh`, kept as public evidence. */
export const DEMO_GUILDS: { id: string; title: string; summary: string }[] = [
  {
    id: 'CBLMFTAKS64G2CTQRJ2D2GPH6OKO7KUMFDXTQWCTPWCKKLHNJY5Z4DOJ',
    title: 'Walk-away: the first recipient stops paying',
    summary: '3 seats, 100% collateral. Rounds 2 and 3 still paid full pots from the walk-away’s escrow and bond.',
  },
  {
    id: 'CBXQ2N4WOW7JN35656YMXJTIJRYGBJOVBOYM7C42VXWB646C52VOO62Y',
    title: 'Covered default before receiving',
    summary: '2 seats. One member never paid; their bond covered it and the recipient got the full pot.',
  },
  {
    id: 'CC4TAGGC2ZGU3HAZAMI57P5XLMOVXO6RGW4VSYZK4BBWWBWNGO2I6EI3',
    title: 'Full cycle, everyone pays',
    summary: '4 seats × 100 USDC: four payouts of exactly 399 USDC, then every bond returned.',
  },
]
