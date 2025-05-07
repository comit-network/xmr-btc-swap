import { Box, Typography } from '@material-ui/core'
import SlideTemplate from './SlideTemplate'
import imagePath from 'assets/groupWithChatbubbles.png' 
import GitHubIcon from "@material-ui/icons/GitHub"
import MatrixIcon from 'renderer/components/icons/MatrixIcon'
import LinkIconButton from 'renderer/components/icons/LinkIconButton'

export default function Slide02_ChooseAMaker(props: slideProps) {
    return (
        <SlideTemplate title="Reach out" {...props} imagePath={imagePath} customContinueButtonText="Get Started">
            <Typography variant="subtitle1">
                We would love to hear about your experience with Unstoppable
                Swap and invite you to join our community.
            </Typography>
            <Box mt={3}>
                <LinkIconButton url="https://github.com/UnstoppableSwap/core">
                    <GitHubIcon/>
                </LinkIconButton>
                <LinkIconButton url="https://matrix.to/#/#unstoppableswap:matrix.org">
                    <MatrixIcon/>
                </LinkIconButton>
            </Box>
        </SlideTemplate>
    )
}
