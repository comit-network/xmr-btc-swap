import { Link, Typography } from '@material-ui/core'
import SlideTemplate from './SlideTemplate'
import imagePath from 'assets/mockHistoryPage.svg'
import ExternalLink from 'renderer/components/other/ExternalLink'

export default function Slide05_KeepAnEyeOnYourSwaps(props: slideProps) {
    return (
        <SlideTemplate
            title="Monitor Your Swaps"
            stepLabel="Step 3"
            {...props}
            imagePath={imagePath}
        >
            <Typography>
                Monitor active swaps to ensure everything proceeds smoothly.
            </Typography>
            <Typography>
                <ExternalLink href='https://docs.unstoppableswap.net/usage/first_swap'>
                    Learn more about atomic swaps
                </ExternalLink>
            </Typography>
        </SlideTemplate>
    )
}
