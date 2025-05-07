import { Typography } from '@material-ui/core'
import SlideTemplate from './SlideTemplate'
import imagePath from 'assets/walletWithBitcoinAndMonero.png'

export default function Slide01_GettingStarted(props: slideProps) {
    return (
        <SlideTemplate
            title="Getting Started"
            {...props}
            imagePath={imagePath}
        >
            <Typography variant="subtitle1">
                To start swapping, you'll need:
            </Typography>
            <Typography>
                <ul>
                    <li>A Bitcoin wallet with funds to swap</li>
                    <li>A Monero wallet to receive your Monero</li>
                </ul>
            </Typography>
        </SlideTemplate>
    )
}
