import { Box, Typography, Paper, Button, Slide } from '@material-ui/core'
import CardSelectionGroup from 'renderer/components/inputs/CardSelection/CardSelectionGroup'
import CardSelectionOption from 'renderer/components/inputs/CardSelection/CardSelectionOption'
import SlideTemplate from './SlideTemplate'
import imagePath from 'assets/currencyFetching.svg'

const FiatPricePreferenceSlide = ({
    handleContinue,
    handlePrevious,
    showFiat,
    onChange,
}: slideProps & {
    showFiat: boolean
    onChange: (value: string) => void
}) => {
    return (
        <SlideTemplate handleContinue={handleContinue} handlePrevious={handlePrevious} title="Fiat Prices" imagePath={imagePath}>
                <Typography variant="subtitle1" color="textSecondary">
                    Do you want to show fiat prices?
                </Typography>
                <CardSelectionGroup
                    value={showFiat ? 'show' : 'hide'}
                    onChange={onChange}
                >
                    <CardSelectionOption value="show">
                        <Typography>Show fiat prices</Typography>
                        <Typography
                            variant="caption"
                            color="textSecondary"
                            paragraph
                            style={{ marginBottom: 4 }}
                        >
                            We connect to CoinGecko to provide realtime currency
                            prices.
                        </Typography>
                    </CardSelectionOption>
                    <CardSelectionOption value="hide">
                        <Typography>Don't show fiat prices</Typography>
                    </CardSelectionOption>
                </CardSelectionGroup>
                <Box style={{ marginTop: "0.5rem" }}>
                    <Typography
                        variant="caption"
                        color="textSecondary"
                    >
                        You can change your preference later in the settings
                    </Typography>
                </Box>
        </SlideTemplate>
    )
}

export default FiatPricePreferenceSlide
