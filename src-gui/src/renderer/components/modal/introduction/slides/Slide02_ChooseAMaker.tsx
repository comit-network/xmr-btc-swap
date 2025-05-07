import { Typography } from '@material-ui/core'
import SlideTemplate from './SlideTemplate'
import imagePath from 'assets/mockMakerSelection.svg'

export default function Slide02_ChooseAMaker(props: slideProps) {
    return (
        <SlideTemplate
            title="Choose a Maker"
            stepLabel="Step 1"
            {...props}
            imagePath={imagePath}
        >
            <Typography variant="subtitle1">
                To start a swap, choose a maker. Each maker offers different exchange rates and limits.
            </Typography>
        </SlideTemplate>
    )
}
